//! Profile packs: composition, cross-document validation and resolution.
//!
//! A pack is a *data file*. It carries work profiles, team templates, the role,
//! skill and context documents they refer to, persona scenarios, and a manifest
//! that says which categories a deployment can actually run.
//!
//! This module owns three things and nothing else:
//!
//! 1. **Composition** — putting the existing KON-MVP-03 documents together.
//! 2. **Cross-document validation** — the checks a single document cannot make
//!    for itself: does this pinned reference resolve, exactly once, at exactly
//!    that revision?
//! 3. **Deterministic revision** — publishing `n+1` without touching `n`.
//!
//! Persistence stays in `kontor-store`; outcome policy stays in `kontor-core`.
//!
//! Every rule below is structural. No function in this crate compares an id to
//! a literal, so the bundled pack and a deployment's own pack take exactly the
//! same path through it.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::TeamRunId;
use kontor_core::id::{
    ArtifactKey, CanonicalDocument, ContentHash, ExternalName, GateKey, PhaseKey, RoleCode,
    RoleKey, SchemaVersion, SkillKey, SpecVersion, TeamTemplateId, Timestamp, TopologyKindKey,
    WorkProfileKey, validate_open_key,
};
use kontor_core::spec::{
    PersonaScenarioSnapshot, PersonaScenarioSpec, ProjectSessionTopologySpec,
    ResolvedWorkProfileSnapshot, RoleCatalogRevision, RoleContextSeed, TeamContextPolicySeed,
    TeamTemplateRevision, WorkProfileSpec,
};
use kontor_core::state::{GateState, TaskClosureCertificate};
use kontor_core::{DomainError, DomainResult};
use kontor_teams::run::TeamClosureCertificate;
use kontor_teams::spec::TeamTemplateSpec;
use serde::{Deserialize, Serialize};

/// Declare an open, deployment-defined key that obeys the one core lexical rule.
macro_rules! pack_keys {
    ($( $(#[$meta:meta])* $name:ident ),+ $(,)?) => {
        $(
            $(#[$meta])*
            ///
            /// An open key. See [`kontor_core::id::validate_open_key`] for the
            /// rule; nothing in this crate enumerates the legal values.
            #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
            #[serde(transparent)]
            pub struct $name(String);

            impl $name {
                /// Parse and validate the key.
                ///
                /// # Errors
                /// See [`kontor_core::id::validate_open_key`].
                pub fn parse(text: &str) -> DomainResult<Self> {
                    validate_open_key(stringify!($name), text)?;
                    Ok(Self(text.to_owned()))
                }

                /// Borrow the key text.
                #[must_use]
                pub fn as_str(&self) -> &str {
                    &self.0
                }
            }

            impl std::fmt::Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str(&self.0)
                }
            }

            impl<'de> Deserialize<'de> for $name {
                fn deserialize<D: serde::Deserializer<'de>>(
                    deserializer: D,
                ) -> Result<Self, D::Error> {
                    use serde::de::Error as _;
                    let text = String::deserialize(deserializer)?;
                    Self::parse(&text).map_err(D::Error::custom)
                }
            }
        )+
    };
}

pack_keys! {
    /// Names one profile pack.
    ProfilePackKey,
    /// Names one category a pack advertises.
    PackCategoryKey,
}

// ---------------------------------------------------------------------------
// Reference documents
// ---------------------------------------------------------------------------

/// One revision of a role definition a profile or slot may pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleDefinition {
    /// The role.
    pub role: RoleKey,
    /// This revision.
    pub version: SpecVersion,
    /// Human label.
    pub label: ExternalName,
}

/// One revision of a skill definition a slot may pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDefinition {
    /// The skill.
    pub skill: SkillKey,
    /// This revision.
    pub version: SpecVersion,
    /// Human label.
    pub label: ExternalName,
}

/// One revision of a context-pack template a slot may pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextDefinition {
    /// The template.
    pub template: ArtifactKey,
    /// This revision.
    pub version: SpecVersion,
    /// Human label.
    pub label: ExternalName,
}

/// Whether a manifest category resolves to a runnable profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackAvailability {
    /// The category is backed by a profile revision in this pack.
    Seeded,
    /// The category advertises vocabulary only. It is deliberately not
    /// runnable, and [`resolve_profile`] refuses it.
    ManifestOnly,
}

/// One category a pack advertises.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackManifestEntry {
    /// The open category id. Data, never a branch condition.
    pub category: PackCategoryKey,
    /// Human label.
    pub label: ExternalName,
    /// Whether the category is backed by a profile.
    pub availability: PackAvailability,
    /// The profile a seeded category resolves to.
    pub profile: Option<WorkProfileKey>,
    /// The pinned revision of that profile.
    pub profile_version: Option<SpecVersion>,
}

/// One persona scenario together with the profile revision it exercises.
///
/// A [`PersonaScenarioSpec`] names a gate but not the profile that declares it,
/// so a pack pins the profile explicitly: guessing which profile a gate belongs
/// to is exactly the kind of implicit resolution a persona safety check must not
/// depend on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackPersona {
    /// The profile whose gate is under test.
    pub profile: WorkProfileKey,
    /// The pinned revision of that profile.
    pub profile_version: SpecVersion,
    /// The scenario itself.
    pub scenario: PersonaScenarioSpec,
}

// ---------------------------------------------------------------------------
// The pack
// ---------------------------------------------------------------------------

/// A versioned bundle of profiles, teams, reference documents and personas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilePackSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// Open pack id.
    pub pack_id: ProfilePackKey,
    /// This revision of the pack.
    pub version: SpecVersion,
    /// What the pack advertises.
    pub manifest: Vec<PackManifestEntry>,
    /// The work profiles it carries.
    pub profiles: Vec<WorkProfileSpec>,
    /// The typed team templates it carries.
    ///
    /// Defaulted so a pack may be composed from a separate team data file, as
    /// the bundled pack is.
    #[serde(default)]
    pub teams: Vec<TeamTemplateSpec>,
    /// Role definitions profiles and slots pin.
    pub roles: Vec<RoleDefinition>,
    /// Skill definitions slots pin.
    pub skills: Vec<SkillDefinition>,
    /// Context templates slots pin.
    #[serde(default)]
    pub contexts: Vec<ContextDefinition>,
    /// Persona scenarios, each bound to the profile it exercises.
    #[serde(default)]
    pub personas: Vec<PackPersona>,
    /// The deployment's context-window seed for each logical role.
    ///
    /// This is the only place a role name meets a context class, and it is data:
    /// nothing in this crate compares a role id to a literal to decide how much
    /// context a seat gets. A pack that seeds nothing leaves every seat on the
    /// standard fallback.
    #[serde(default)]
    pub role_context_seeds: Vec<RoleContextSeed>,
}

/// How one Foundation role slot is spelled as a standard catalog role.
///
/// Seeded data rather than a rule in code. The two vocabularies are genuinely
/// separate — a Foundation slot is an open deployment-defined key, a catalog
/// code is a closed standard one — and a daemon that carried the correspondence
/// as a `match` would hold a copy of it no seed revision could correct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryRoleBinding {
    /// The Foundation role a team declares.
    pub role: RoleKey,
    /// The standard catalog code the seat is recorded under.
    pub role_code: RoleCode,
}

/// Which topology kinds carry delivery, and how delivery roles are spelled.
///
/// Every kind here is named as *data*. Several kinds in the bundled vocabulary
/// are `native_child` session hosts below an epic, so "which one serves a task"
/// is not derivable from the capability set — it is a choice the specification
/// data makes, and reading it from here is what keeps the choice correctable
/// without a code change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationalDelivery {
    /// The kind one epic materializes as.
    pub epic_kind: TopologyKindKey,
    /// The kind one delivery task's session host materializes as.
    pub task_kind: TopologyKindKey,
    /// The Foundation-to-catalog role correspondence for delivery seats.
    pub role_bindings: Vec<DeliveryRoleBinding>,
}

impl OperationalDelivery {
    /// The catalog code one Foundation role is recorded under, if it has one.
    ///
    /// A role with no binding returns `None`, and the caller refuses rather
    /// than inventing a code: a seat recorded under a guessed standard role is
    /// worse evidence than a seat that visibly could not be placed.
    #[must_use]
    pub fn role_code(&self, role: &RoleKey) -> Option<&RoleCode> {
        self.role_bindings
            .iter()
            .find(|binding| &binding.role == role)
            .map(|binding| &binding.role_code)
    }
}

/// The Operational domain data bundled independently of Foundation profiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationalDomainPack {
    /// Schema generation of this data file.
    pub schema_version: SchemaVersion,
    /// Generic topology specification revisions.
    pub topology_specs: Vec<ProjectSessionTopologySpec>,
    /// Server-owned standard-role catalog revisions.
    pub role_catalogs: Vec<RoleCatalogRevision>,
    /// How delivery work is placed in the topology this data declares.
    pub delivery: OperationalDelivery,
}

impl OperationalDomainPack {
    /// Validate every document and prove revision identities are unique.
    ///
    /// # Errors
    /// As the contained specifications, plus duplicate identities.
    pub fn validate(&self) -> DomainResult<()> {
        let mut topologies = BTreeSet::new();
        for topology in &self.topology_specs {
            topology.validate()?;
            if !topologies.insert((topology.spec_id, topology.version)) {
                return Err(DomainError::invalid(
                    "OperationalDomainPack",
                    "declares a duplicate topology specification revision",
                ));
            }
        }
        let mut catalogs = BTreeSet::new();
        for catalog in &self.role_catalogs {
            catalog.validate()?;
            if !catalogs.insert((catalog.catalog_id, catalog.version)) {
                return Err(DomainError::invalid(
                    "OperationalDomainPack",
                    "declares a duplicate role catalog revision",
                ));
            }
        }
        if self.topology_specs.is_empty() || self.role_catalogs.is_empty() {
            return Err(DomainError::invalid(
                "OperationalDomainPack",
                "must carry topology and role-catalog data",
            ));
        }
        // The delivery binding is only usable if every code and kind it names is
        // actually declared here. Validating it against the same document is
        // what stops a seed revision from pointing delivery at a kind or a role
        // that this vocabulary does not have.
        let topology = &self.topology_specs[0];
        for kind in [&self.delivery.epic_kind, &self.delivery.task_kind] {
            if !topology.node_kinds.iter().any(|node| &node.kind == kind) {
                return Err(DomainError::invalid(
                    "OperationalDomainPack",
                    "delivery names a topology kind the specification does not declare",
                ));
            }
        }
        let catalog = &self.role_catalogs[0];
        for binding in &self.delivery.role_bindings {
            if catalog.role(&binding.role_code).is_none() {
                return Err(DomainError::invalid(
                    "OperationalDomainPack",
                    "delivery names a role code the catalog does not declare",
                ));
            }
        }
        Ok(())
    }
}

/// Parse and validate Operational domain data.
///
/// # Errors
/// Returns [`DomainError`] when the document shape or any contained revision is
/// invalid.
pub fn parse_operational_domain_pack(json: &str) -> DomainResult<OperationalDomainPack> {
    let pack: OperationalDomainPack = serde_json::from_str(json).map_err(|_| {
        DomainError::invalid(
            "OperationalDomainPack",
            "is not a valid Operational domain document",
        )
    })?;
    pack.validate()?;
    Ok(pack)
}

impl ProfilePackSpec {
    /// Look up one profile revision.
    #[must_use]
    pub fn profile(&self, id: &WorkProfileKey, version: SpecVersion) -> Option<&WorkProfileSpec> {
        self.profiles
            .iter()
            .find(|profile| &profile.id == id && profile.version == version)
    }

    /// Look up one team template revision.
    #[must_use]
    pub fn team(&self, id: TeamTemplateId, version: SpecVersion) -> Option<&TeamTemplateSpec> {
        self.teams
            .iter()
            .find(|team| team.template_id == id && team.version == version)
    }

    /// Look up one manifest entry.
    #[must_use]
    pub fn category(&self, category: &PackCategoryKey) -> Option<&PackManifestEntry> {
        self.manifest
            .iter()
            .find(|entry| &entry.category == category)
    }

    /// The context-window resolution inputs a run freezes for one profile.
    ///
    /// The profile's own default plus the seeds for exactly the roles named,
    /// so the frozen inputs describe that run rather than the whole deployment
    /// catalogue. Seeds for roles the run does not use are left out.
    #[must_use]
    pub fn context_policy_for(
        &self,
        profile: &WorkProfileSpec,
        roles: &BTreeSet<RoleKey>,
    ) -> TeamContextPolicySeed {
        TeamContextPolicySeed {
            work_profile: profile.context_window,
            role_seeds: self
                .role_context_seeds
                .iter()
                .filter(|seed| roles.contains(&seed.role))
                .cloned()
                .collect(),
        }
    }

    /// Every category that actually resolves to a profile.
    #[must_use]
    pub fn runnable_categories(&self) -> Vec<&PackCategoryKey> {
        self.manifest
            .iter()
            .filter(|entry| entry.availability == PackAvailability::Seeded)
            .map(|entry| &entry.category)
            .collect()
    }
}

/// The forward-reachability view a cross-document check needs.
///
/// [`WorkProfileSpec::validate`] already proved the graph is a DAG with one
/// entry and reachable terminals; this only answers "is A a forward ancestor of
/// B", which is what makes a producer/consumer ordering check possible.
struct PhaseReach {
    ancestors: BTreeMap<PhaseKey, BTreeSet<PhaseKey>>,
}

impl PhaseReach {
    fn of(profile: &WorkProfileSpec) -> Self {
        let mut predecessors: BTreeMap<PhaseKey, BTreeSet<PhaseKey>> = profile
            .phases
            .iter()
            .map(|phase| (phase.id.clone(), BTreeSet::new()))
            .collect();
        for edge in &profile.edges {
            if let Some(set) = predecessors.get_mut(&edge.to) {
                set.insert(edge.from.clone());
            }
        }
        let mut ancestors: BTreeMap<PhaseKey, BTreeSet<PhaseKey>> = BTreeMap::new();
        for phase in &profile.phases {
            let mut seen: BTreeSet<PhaseKey> = BTreeSet::new();
            let mut stack: Vec<PhaseKey> = predecessors
                .get(&phase.id)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default();
            while let Some(current) = stack.pop() {
                if !seen.insert(current.clone()) {
                    continue;
                }
                if let Some(set) = predecessors.get(&current) {
                    stack.extend(set.iter().cloned());
                }
            }
            ancestors.insert(phase.id.clone(), seen);
        }
        Self { ancestors }
    }

    /// Whether `producer` can have run by the time `consumer` runs.
    fn produced_before(&self, producer: &PhaseKey, consumer: &PhaseKey) -> bool {
        producer == consumer
            || self
                .ancestors
                .get(consumer)
                .is_some_and(|set| set.contains(producer))
    }
}

/// Validate the whole pack and every pinned cross-reference in it.
///
/// On top of each document's own validation this proves:
///
/// * every `(id, version)` identity in the pack is unique;
/// * every pinned role, skill, context, team and profile reference resolves
///   exactly once, at exactly the pinned revision;
/// * a gate a phase lists belongs to that phase, and a phase lists every gate
///   that belongs to it;
/// * an artifact's producing phase is the consuming phase or a forward ancestor
///   of it, for phase requirements, gate evidence and scenario evidence alike;
/// * every `handoff_role` on a phase edge is declared by the profile *and*
///   supplied by at least one slot of the team it pinned;
/// * every gate evaluator and waiver role is carried by the pinned team's
///   authority for that exact gate;
/// * a team handoff's phase and artifacts exist in the profile that pinned it;
/// * a persona's gate exists, its actor holds no authority over that gate in
///   either the profile or the team, and its evaluators are independently
///   authorized.
///
/// # Errors
/// Returns the first [`DomainError`]. Errors name the rule, never the offending
/// document content.
pub fn validate_pack(pack: &ProfilePackSpec) -> DomainResult<()> {
    let catalog = Catalog::build(pack)?;
    for profile in &pack.profiles {
        profile.validate()?;
        validate_profile_references(pack, profile, &catalog)?;
    }
    for team in &pack.teams {
        team.validate()?;
        validate_team_references(team, &catalog)?;
    }
    validate_manifest(pack)?;
    for persona in &pack.personas {
        validate_persona(pack, persona)?;
    }
    Ok(())
}

/// The pack's reference documents, indexed by pinned identity.
struct Catalog {
    roles: BTreeSet<(RoleKey, SpecVersion)>,
    skills: BTreeSet<(SkillKey, SpecVersion)>,
    contexts: BTreeSet<(ArtifactKey, SpecVersion)>,
}

impl Catalog {
    fn build(pack: &ProfilePackSpec) -> DomainResult<Self> {
        let mut roles = BTreeSet::new();
        for definition in &pack.roles {
            if !roles.insert((definition.role.clone(), definition.version)) {
                return Err(DomainError::invalid(
                    "ProfilePackSpec",
                    "declares a role revision twice",
                ));
            }
        }
        let mut skills = BTreeSet::new();
        for definition in &pack.skills {
            if !skills.insert((definition.skill.clone(), definition.version)) {
                return Err(DomainError::invalid(
                    "ProfilePackSpec",
                    "declares a skill revision twice",
                ));
            }
        }
        let mut contexts = BTreeSet::new();
        for definition in &pack.contexts {
            if !contexts.insert((definition.template.clone(), definition.version)) {
                return Err(DomainError::invalid(
                    "ProfilePackSpec",
                    "declares a context template revision twice",
                ));
            }
        }

        let mut profiles = BTreeSet::new();
        for profile in &pack.profiles {
            if !profiles.insert((profile.id.clone(), profile.version)) {
                return Err(DomainError::invalid(
                    "ProfilePackSpec",
                    "declares a work profile revision twice",
                ));
            }
        }
        let mut teams = BTreeSet::new();
        for team in &pack.teams {
            if !teams.insert((team.template_id, team.version)) {
                return Err(DomainError::invalid(
                    "ProfilePackSpec",
                    "declares a team template revision twice",
                ));
            }
        }
        let mut scenarios = BTreeSet::new();
        for persona in &pack.personas {
            if !scenarios.insert((persona.scenario.scenario_id, persona.scenario.version)) {
                return Err(DomainError::invalid(
                    "ProfilePackSpec",
                    "declares a persona scenario revision twice",
                ));
            }
        }

        Ok(Self {
            roles,
            skills,
            contexts,
        })
    }

    fn has_role(&self, role: &RoleKey, version: SpecVersion) -> bool {
        self.roles.contains(&(role.clone(), version))
    }

    fn has_skill(&self, skill: &SkillKey, version: SpecVersion) -> bool {
        self.skills.contains(&(skill.clone(), version))
    }

    fn has_context(&self, template: &ArtifactKey, version: SpecVersion) -> bool {
        self.contexts.contains(&(template.clone(), version))
    }
}

fn validate_manifest(pack: &ProfilePackSpec) -> DomainResult<()> {
    let mut seen: BTreeSet<&PackCategoryKey> = BTreeSet::new();
    for entry in &pack.manifest {
        if !seen.insert(&entry.category) {
            return Err(DomainError::invalid(
                "ProfilePackSpec",
                "advertises a category twice",
            ));
        }
        match entry.availability {
            PackAvailability::Seeded => {
                let (Some(id), Some(version)) = (&entry.profile, entry.profile_version) else {
                    return Err(DomainError::invalid(
                        "ProfilePackSpec",
                        "a seeded category must pin a profile revision",
                    ));
                };
                if pack.profile(id, version).is_none() {
                    return Err(DomainError::invalid(
                        "ProfilePackSpec",
                        "a seeded category pins a profile revision the pack does not carry",
                    ));
                }
            }
            PackAvailability::ManifestOnly => {
                if entry.profile.is_some() || entry.profile_version.is_some() {
                    return Err(DomainError::invalid(
                        "ProfilePackSpec",
                        "a manifest-only category must not pin a profile revision",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_profile_references(
    pack: &ProfilePackSpec,
    profile: &WorkProfileSpec,
    catalog: &Catalog,
) -> DomainResult<()> {
    for reference in &profile.roles {
        if !catalog.has_role(&reference.role, reference.version) {
            return Err(DomainError::invalid(
                "ProfilePackSpec",
                "a profile pins a role revision the pack does not carry",
            ));
        }
    }
    for reference in &profile.skills {
        if !catalog.has_skill(&reference.skill, reference.version) {
            return Err(DomainError::invalid(
                "ProfilePackSpec",
                "a profile pins a skill revision the pack does not carry",
            ));
        }
    }

    // A phase must own exactly the gates that name it. The core validator
    // proves each listed gate exists; this proves the listing and the gate's own
    // `phase` field agree in both directions, so a gate cannot be evaluated at a
    // phase that never mentions it.
    for phase in &profile.phases {
        for gate in &phase.gates {
            let declared = profile.gate(gate).ok_or(DomainError::Invalid {
                subject: "ProfilePackSpec",
                rule: "a phase lists a gate the profile does not declare",
            })?;
            if declared.phase != phase.id {
                return Err(DomainError::invalid(
                    "ProfilePackSpec",
                    "a phase lists a gate that belongs to another phase",
                ));
            }
        }
    }
    for gate in &profile.gates {
        let owner = profile
            .phases
            .iter()
            .find(|phase| phase.id == gate.phase)
            .ok_or(DomainError::Invalid {
                subject: "ProfilePackSpec",
                rule: "a gate names a phase the profile does not declare",
            })?;
        if !owner.gates.contains(&gate.id) {
            return Err(DomainError::invalid(
                "ProfilePackSpec",
                "a gate's own phase does not list it",
            ));
        }
    }

    let reach = PhaseReach::of(profile);
    let producers: BTreeMap<&ArtifactKey, &PhaseKey> = profile
        .artifacts
        .iter()
        .map(|contract| (&contract.key, &contract.producer_phase))
        .collect();
    for phase in &profile.phases {
        for required in &phase.required_artifacts {
            ensure_produced_before(&producers, &reach, required, &phase.id)?;
        }
    }
    for gate in &profile.gates {
        for evidence in &gate.required_evidence {
            ensure_produced_before(&producers, &reach, evidence, &gate.phase)?;
        }
    }

    let Some(pinned) = &profile.team_template else {
        // A profile that pins no team declares no team obligations. Its handoff
        // roles still have to be roles it declared, which the core validator
        // does not check because a handoff role is optional there.
        return ensure_handoff_roles_declared(profile);
    };
    ensure_handoff_roles_declared(profile)?;

    let team = pack
        .team(pinned.template_id, pinned.version)
        .ok_or(DomainError::Invalid {
            subject: "ProfilePackSpec",
            rule: "a profile pins a team template revision the pack does not carry",
        })?;

    // Every role the profile hands work to must be a seat somebody actually
    // occupies. A handoff to a role no slot fills is work that cannot start.
    for edge in &profile.edges {
        if let Some(role) = &edge.handoff_role
            && team.cardinality_of(role) == 0
        {
            return Err(DomainError::invalid(
                "ProfilePackSpec",
                "a profile hands off to a role the pinned team supplies no slot for",
            ));
        }
    }

    // The team may serve several profiles and therefore carry authority over
    // gates this profile never declares. The obligation runs the other way: every
    // gate *this* profile declares must be covered by the pinned team.
    let derived = team.role_authority();
    for gate in &profile.gates {
        for role in &gate.evaluator_roles {
            let carried = derived
                .iter()
                .find(|entry| &entry.role == role)
                .is_some_and(|entry| entry.may_evaluate.contains(&gate.id));
            if !carried {
                return Err(DomainError::MissingAuthority {
                    subject: "profile pack",
                    rule: "the pinned team does not authorize an evaluator of a profile gate",
                });
            }
        }
        for role in &gate.waiver_roles {
            let carried = derived
                .iter()
                .find(|entry| &entry.role == role)
                .is_some_and(|entry| entry.may_waive.contains(&gate.id));
            if !carried {
                return Err(DomainError::MissingAuthority {
                    subject: "profile pack",
                    rule: "the pinned team does not authorize a waiver role of a profile gate",
                });
            }
        }
    }

    let phases: BTreeSet<&PhaseKey> = profile.phases.iter().map(|phase| &phase.id).collect();
    for handoff in &team.handoffs {
        if let Some(after) = &handoff.after_phase
            && !phases.contains(after)
        {
            return Err(DomainError::invalid(
                "ProfilePackSpec",
                "a team handoff names a phase the pinned profile does not declare",
            ));
        }
        for artifact in &handoff.required_artifacts {
            if !producers.contains_key(artifact) {
                return Err(DomainError::invalid(
                    "ProfilePackSpec",
                    "a team handoff requires an artifact the pinned profile does not declare",
                ));
            }
        }
    }

    Ok(())
}

fn ensure_handoff_roles_declared(profile: &WorkProfileSpec) -> DomainResult<()> {
    let declared: BTreeSet<&RoleKey> = profile.roles.iter().map(|entry| &entry.role).collect();
    for edge in &profile.edges {
        if let Some(role) = &edge.handoff_role
            && !declared.contains(role)
        {
            return Err(DomainError::invalid(
                "ProfilePackSpec",
                "a phase edge hands off to a role the profile does not declare",
            ));
        }
    }
    Ok(())
}

fn ensure_produced_before(
    producers: &BTreeMap<&ArtifactKey, &PhaseKey>,
    reach: &PhaseReach,
    artifact: &ArtifactKey,
    consumer: &PhaseKey,
) -> DomainResult<()> {
    let producer = producers.get(artifact).ok_or(DomainError::Invalid {
        subject: "ProfilePackSpec",
        rule: "an artifact reference has no producing contract",
    })?;
    if !reach.produced_before(producer, consumer) {
        return Err(DomainError::invalid(
            "ProfilePackSpec",
            "an artifact is consumed before the phase that produces it can run",
        ));
    }
    Ok(())
}

fn validate_team_references(team: &TeamTemplateSpec, catalog: &Catalog) -> DomainResult<()> {
    for requirement in &team.roles {
        if !catalog.has_role(&requirement.role.role, requirement.role.version) {
            return Err(DomainError::invalid(
                "ProfilePackSpec",
                "a team requires a role revision the pack does not carry",
            ));
        }
    }
    for slot in &team.slots {
        for skill in &slot.skills {
            if !catalog.has_skill(&skill.skill, skill.version) {
                return Err(DomainError::invalid(
                    "ProfilePackSpec",
                    "a role slot pins a skill revision the pack does not carry",
                ));
            }
        }
        if let Some(context) = &slot.context
            && !catalog.has_context(&context.template, context.version)
        {
            return Err(DomainError::invalid(
                "ProfilePackSpec",
                "a role slot pins a context template revision the pack does not carry",
            ));
        }
    }
    Ok(())
}

fn validate_persona(pack: &ProfilePackSpec, persona: &PackPersona) -> DomainResult<()> {
    persona.scenario.validate()?;
    let profile = pack
        .profile(&persona.profile, persona.profile_version)
        .ok_or(DomainError::Invalid {
            subject: "ProfilePackSpec",
            rule: "a persona pins a profile revision the pack does not carry",
        })?;
    let gate = profile
        .gate(&persona.scenario.gate_under_test)
        .ok_or(DomainError::Invalid {
            subject: "ProfilePackSpec",
            rule: "a persona exercises a gate the pinned profile does not declare",
        })?;

    // The simulated actor may not sign off its own scenario in any form.
    if gate.evaluator_roles.contains(&persona.scenario.actor_role)
        || gate.waiver_roles.contains(&persona.scenario.actor_role)
    {
        return Err(DomainError::MissingAuthority {
            subject: "persona scenario",
            rule: "the simulated persona must not hold authority over its own gate",
        });
    }
    for role in &persona.scenario.evaluator_roles {
        if !gate.evaluator_roles.contains(role) {
            return Err(DomainError::MissingAuthority {
                subject: "persona scenario",
                rule: "an evaluator is not authorized by the pinned gate",
            });
        }
        if gate.waiver_roles.contains(role) {
            return Err(DomainError::MissingAuthority {
                subject: "persona scenario",
                rule: "evaluator and waiver authority must not overlap",
            });
        }
    }

    // ...and not through the team either: a seat that can decide the gate is
    // authority just as much as a gate role is.
    if let Some(pinned) = &profile.team_template
        && let Some(team) = pack.team(pinned.template_id, pinned.version)
    {
        let holds = team.role_authority().into_iter().any(|entry| {
            entry.role == persona.scenario.actor_role
                && (entry.may_evaluate.contains(&gate.id) || entry.may_waive.contains(&gate.id))
        });
        if holds {
            return Err(DomainError::MissingAuthority {
                subject: "persona scenario",
                rule: "the pinned team gives the simulated persona authority over its own gate",
            });
        }
    }

    let reach = PhaseReach::of(profile);
    let producers: BTreeMap<&ArtifactKey, &PhaseKey> = profile
        .artifacts
        .iter()
        .map(|contract| (&contract.key, &contract.producer_phase))
        .collect();
    for evidence in &persona.scenario.required_evidence {
        ensure_produced_before(&producers, &reach, evidence, &gate.phase)?;
    }
    for step in &persona.scenario.steps {
        for evidence in &step.expected_evidence {
            ensure_produced_before(&producers, &reach, evidence, &gate.phase)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// One category resolved into everything a run needs, owned outright.
///
/// The bundle keeps no handle back into the pack it came from, so a later edit
/// to the pack cannot change what a run resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedProfileBundle {
    /// Schema generation of this bundle.
    pub schema_version: SchemaVersion,
    /// The category that was resolved.
    pub category: PackCategoryKey,
    /// The pack revision it came from.
    pub pack_id: ProfilePackKey,
    /// That pack's revision.
    pub pack_version: SpecVersion,
    /// The frozen profile, exactly as KON-MVP-03 spells it.
    pub profile: ResolvedWorkProfileSnapshot,
    /// The pinned team revision, if the profile prescribes one.
    pub team: Option<TeamTemplateRevision>,
    /// The role documents this run selected.
    pub roles: Vec<RoleDefinition>,
    /// The skill documents this run selected.
    pub skills: Vec<SkillDefinition>,
    /// The context templates this run selected.
    pub contexts: Vec<ContextDefinition>,
    /// The context-window resolution inputs this run freezes: the profile's own
    /// default and the seeds for the roles it actually selected.
    pub context_policy: TeamContextPolicySeed,
    /// Digest of everything above.
    pub bundle_hash: ContentHash,
}

/// The exact shape [`ResolvedProfileBundle::bundle_hash`] covers.
#[derive(Debug, Serialize)]
struct BundleDigestInput<'a> {
    schema_version: SchemaVersion,
    category: &'a PackCategoryKey,
    pack_id: &'a ProfilePackKey,
    pack_version: SpecVersion,
    profile: &'a ResolvedWorkProfileSnapshot,
    team: Option<&'a TeamTemplateRevision>,
    roles: &'a [RoleDefinition],
    skills: &'a [SkillDefinition],
    contexts: &'a [ContextDefinition],
    context_policy: &'a TeamContextPolicySeed,
}

impl ResolvedProfileBundle {
    /// Re-derive the digest and prove nothing in the bundle moved.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the profile no longer matches its own pinned
    /// hash, when the team revision no longer matches its definition, or when
    /// the bundle no longer matches `bundle_hash`.
    pub fn verify(&self) -> DomainResult<()> {
        self.profile.verify()?;
        if let Some(team) = &self.team {
            TeamTemplateSpec::from_revision(team)?;
        }
        if self.digest()? != self.bundle_hash {
            return Err(DomainError::invalid(
                "ResolvedProfileBundle",
                "no longer matches its pinned digest",
            ));
        }
        Ok(())
    }

    fn digest(&self) -> DomainResult<ContentHash> {
        Ok(CanonicalDocument::from_serializable(&BundleDigestInput {
            schema_version: self.schema_version,
            category: &self.category,
            pack_id: &self.pack_id,
            pack_version: self.pack_version,
            profile: &self.profile,
            team: self.team.as_ref(),
            roles: &self.roles,
            skills: &self.skills,
            contexts: &self.contexts,
            context_policy: &self.context_policy,
        })?
        .hash()
        .clone())
    }

    /// Freeze a persona scenario onto this bundle's profile.
    ///
    /// Reuses the existing KON-MVP-03 authority proof rather than restating it,
    /// so a scenario is judged against the gate the *pinned* profile declares.
    ///
    /// # Errors
    /// As [`PersonaScenarioSnapshot::freeze_onto_task`].
    pub fn freeze_persona(
        &self,
        scenario: &PersonaScenarioSpec,
    ) -> DomainResult<PersonaScenarioSnapshot> {
        PersonaScenarioSnapshot::freeze_onto_task(scenario, &self.profile)
    }
}

/// Resolve one advertised category into an owned, hashed bundle.
///
/// A manifest-only category is refused here: advertising vocabulary is not the
/// same as shipping a runnable workflow, and the difference has to be visible at
/// the moment somebody tries to run one.
///
/// # Errors
/// * [`DomainError::Invalid`] for an unknown category, a manifest-only category
///   and any pinned reference the pack does not carry.
/// * Whatever [`validate_pack`] returns.
pub fn resolve_profile(
    pack: &ProfilePackSpec,
    category: &PackCategoryKey,
    resolved_at: Timestamp,
) -> DomainResult<ResolvedProfileBundle> {
    validate_pack(pack)?;

    let entry = pack.category(category).ok_or(DomainError::Invalid {
        subject: "ProfilePackSpec",
        rule: "the pack advertises no such category",
    })?;
    if entry.availability != PackAvailability::Seeded {
        return Err(DomainError::invalid(
            "ProfilePackSpec",
            "a manifest-only category advertises vocabulary and is not runnable",
        ));
    }
    let (Some(id), Some(version)) = (&entry.profile, entry.profile_version) else {
        return Err(DomainError::invalid(
            "ProfilePackSpec",
            "a seeded category must pin a profile revision",
        ));
    };
    let definition = pack.profile(id, version).ok_or(DomainError::Invalid {
        subject: "ProfilePackSpec",
        rule: "a seeded category pins a profile revision the pack does not carry",
    })?;

    let profile = ResolvedWorkProfileSnapshot::resolve(definition, resolved_at)?;
    let team = match &definition.team_template {
        Some(pinned) => {
            let spec =
                pack.team(pinned.template_id, pinned.version)
                    .ok_or(DomainError::Invalid {
                        subject: "ProfilePackSpec",
                        rule: "a profile pins a team template revision the pack does not carry",
                    })?;
            Some(spec.to_revision()?)
        }
        None => None,
    };

    // The selection is the union of what the profile pins and what the team's
    // slots pin, deduplicated and ordered so the digest is stable.
    let mut role_pins: BTreeSet<(RoleKey, SpecVersion)> = definition
        .roles
        .iter()
        .map(|reference| (reference.role.clone(), reference.version))
        .collect();
    let mut skill_pins: BTreeSet<(SkillKey, SpecVersion)> = definition
        .skills
        .iter()
        .map(|reference| (reference.skill.clone(), reference.version))
        .collect();
    let mut context_pins: BTreeSet<(ArtifactKey, SpecVersion)> = BTreeSet::new();
    if let Some(pinned) = &definition.team_template
        && let Some(spec) = pack.team(pinned.template_id, pinned.version)
    {
        for requirement in &spec.roles {
            role_pins.insert((requirement.role.role.clone(), requirement.role.version));
        }
        for slot in &spec.slots {
            for skill in &slot.skills {
                skill_pins.insert((skill.skill.clone(), skill.version));
            }
            if let Some(context) = &slot.context {
                context_pins.insert((context.template.clone(), context.version));
            }
        }
    }

    let roles = select(&pack.roles, &role_pins, |definition| {
        (definition.role.clone(), definition.version)
    })?;
    let skills = select(&pack.skills, &skill_pins, |definition| {
        (definition.skill.clone(), definition.version)
    })?;
    let contexts = select(&pack.contexts, &context_pins, |definition| {
        (definition.template.clone(), definition.version)
    })?;

    let selected_roles: BTreeSet<RoleKey> = roles.iter().map(|role| role.role.clone()).collect();
    let context_policy = pack.context_policy_for(definition, &selected_roles);
    context_policy.validate()?;

    let mut bundle = ResolvedProfileBundle {
        schema_version: definition.schema_version,
        category: category.clone(),
        pack_id: pack.pack_id.clone(),
        pack_version: pack.version,
        profile,
        team,
        roles,
        skills,
        contexts,
        context_policy,
        bundle_hash: ContentHash::of(b""),
    };
    bundle.bundle_hash = bundle.digest()?;
    Ok(bundle)
}

/// Copy out exactly the pinned documents, in a deterministic order.
fn select<T: Clone, K: Ord>(
    available: &[T],
    pins: &BTreeSet<K>,
    identity: impl Fn(&T) -> K,
) -> DomainResult<Vec<T>> {
    let mut selected: BTreeMap<K, T> = BTreeMap::new();
    for candidate in available {
        let key = identity(candidate);
        if pins.contains(&key) {
            selected.insert(key, candidate.clone());
        }
    }
    if selected.len() != pins.len() {
        return Err(DomainError::invalid(
            "ProfilePackSpec",
            "a pinned reference document is missing from the pack",
        ));
    }
    Ok(selected.into_values().collect())
}

// ---------------------------------------------------------------------------
// Deterministic revision
// ---------------------------------------------------------------------------

/// Publish the next revision of a work profile without touching the previous
/// one.
///
/// # Errors
/// * [`DomainError::Invalid`] when the edit changed the profile id or the
///   version, or when the revised profile does not validate.
/// * Version overflow, from [`SpecVersion::next`].
pub fn revise_work_profile<F>(previous: &WorkProfileSpec, edit: F) -> DomainResult<WorkProfileSpec>
where
    F: FnOnce(&mut WorkProfileSpec),
{
    let expected = previous.version.next()?;
    let mut revised = previous.clone();
    revised.version = expected;
    edit(&mut revised);
    if revised.id != previous.id {
        return Err(DomainError::invalid(
            "WorkProfileSpec",
            "a revision must preserve the profile's logical id",
        ));
    }
    if revised.version != expected {
        return Err(DomainError::invalid(
            "WorkProfileSpec",
            "a revision must publish exactly the next version",
        ));
    }
    revised.validate()?;
    Ok(revised)
}

/// Publish the next revision of a persona scenario without touching the
/// previous one.
///
/// # Errors
/// * [`DomainError::Invalid`] when the edit changed the scenario id or the
///   version, or when the revised scenario does not validate.
/// * Version overflow, from [`SpecVersion::next`].
pub fn revise_persona_scenario<F>(
    previous: &PersonaScenarioSpec,
    edit: F,
) -> DomainResult<PersonaScenarioSpec>
where
    F: FnOnce(&mut PersonaScenarioSpec),
{
    let expected = previous.version.next()?;
    let mut revised = previous.clone();
    revised.version = expected;
    edit(&mut revised);
    if revised.scenario_id != previous.scenario_id {
        return Err(DomainError::invalid(
            "PersonaScenarioSpec",
            "a revision must preserve the scenario's logical id",
        ));
    }
    if revised.version != expected {
        return Err(DomainError::invalid(
            "PersonaScenarioSpec",
            "a revision must publish exactly the next version",
        ));
    }
    revised.validate()?;
    Ok(revised)
}

// ---------------------------------------------------------------------------
// Task closure
// ---------------------------------------------------------------------------

/// An authorized waiver of one gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateWaiver {
    /// The gate being waived.
    pub gate: GateKey,
    /// The role that waived it.
    pub authorized_by: RoleKey,
    /// Evidence the waiver cites.
    pub evidence: Vec<ArtifactKey>,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// What a task presents about the team that did its work.
///
/// A task's profile and its team are two independent sets of obligations: the
/// profile says which phases, gates and artifacts must exist, and the team says
/// which seats must have finished. Satisfying one says nothing about the other,
/// so closing a task requires both to be presented together.
#[derive(Debug, Clone, Copy)]
pub enum TaskTeamEvidence<'a> {
    /// The task's profile prescribes a team, and this certificate proves every
    /// declared role slot closed with evidence or was excused by an authorized,
    /// evidence-bearing waiver.
    Certified {
        /// The team run the task ran through.
        team_run_id: TeamRunId,
        /// The proof, obtainable only from
        /// [`kontor_teams::run::TeamRunSlots::certify_team_closure`].
        certificate: &'a TeamClosureCertificate,
    },
    /// The task's profile prescribes no team, so there are no seats to account
    /// for. Presenting this for a profile that *does* pin a team is refused.
    NoTeam,
}

/// Certify that a task may close under its pinned profile *and* its team.
///
/// Phase, gate and artifact mechanics are delegated wholesale to the pinned
/// [`ResolvedWorkProfileSnapshot`]. On top of that this function requires two
/// things the generic envelope cannot express:
///
/// 1. a waived gate needs a *named, authorized* waiver carrying the gate's
///    evidence, not merely a `waived` state;
/// 2. a task whose profile prescribes a team needs that team's closure
///    certificate — otherwise a task could reach a terminal state while a role
///    slot still held a live session.
///
/// It follows that a profile which declares more obligations than one discipline
/// covers cannot be closed by that discipline's verdict alone. That is a
/// structural consequence of the profile's own graph; no id is consulted to
/// reach it.
///
/// # Errors
/// * [`DomainError::MissingEvidence`] when a profile that prescribes a team is
///   closed without its team certificate, and when a waiver omits a gate's
///   evidence.
/// * [`DomainError::MissingAuthority`] when a gate is waived without an
///   authorized waiver, or by a role the gate does not authorize.
/// * [`DomainError::Invalid`] when the certificate names another team run, when
///   team evidence is presented for a profile that prescribes no team, and when
///   a waiver names an unknown gate or a gate that is not waived.
/// * Whatever [`ResolvedWorkProfileSnapshot::certify_closure`] returns.
pub fn certify_task_closure(
    profile: &ResolvedWorkProfileSnapshot,
    team: TaskTeamEvidence<'_>,
    completed_phases: &BTreeSet<PhaseKey>,
    gate_states: &BTreeMap<GateKey, GateState>,
    produced_artifacts: &BTreeSet<ArtifactKey>,
    waivers: &[GateWaiver],
) -> DomainResult<TaskClosureCertificate> {
    profile.verify()?;

    match (&profile.definition.team_template, team) {
        (Some(_), TaskTeamEvidence::NoTeam) => {
            return Err(DomainError::MissingEvidence {
                subject: "task closure",
                rule: "a task whose profile prescribes a team must present that team's closure",
            });
        }
        (None, TaskTeamEvidence::Certified { .. }) => {
            return Err(DomainError::invalid(
                "task closure",
                "team closure was presented for a profile that prescribes no team",
            ));
        }
        (
            Some(_),
            TaskTeamEvidence::Certified {
                team_run_id,
                certificate,
            },
        ) => {
            if certificate.team_run_id() != team_run_id {
                return Err(DomainError::invalid(
                    "task closure",
                    "the team certificate proves a different team run",
                ));
            }
        }
        (None, TaskTeamEvidence::NoTeam) => {}
    }

    let mut by_gate: BTreeMap<&GateKey, &GateWaiver> = BTreeMap::new();
    for waiver in waivers {
        let gate = profile
            .definition
            .gate(&waiver.gate)
            .ok_or(DomainError::Invalid {
                subject: "task closure",
                rule: "a waiver names a gate the pinned profile does not declare",
            })?;
        if by_gate.insert(&waiver.gate, waiver).is_some() {
            return Err(DomainError::invalid(
                "task closure",
                "a gate is waived more than once",
            ));
        }
        if gate_states.get(&waiver.gate).copied() != Some(GateState::Waived) {
            return Err(DomainError::invalid(
                "task closure",
                "a waiver names a gate that is not in the waived state",
            ));
        }
        if !gate.waiver_allowed {
            return Err(DomainError::MissingAuthority {
                subject: "task closure",
                rule: "a gate was waived that the profile forbids waiving",
            });
        }
        if !gate.waiver_roles.contains(&waiver.authorized_by) {
            return Err(DomainError::MissingAuthority {
                subject: "task closure",
                rule: "the waiving role is not authorized by the gate",
            });
        }
        let cited: BTreeSet<&ArtifactKey> = waiver.evidence.iter().collect();
        if !gate
            .required_evidence
            .iter()
            .all(|required| cited.contains(required))
        {
            return Err(DomainError::MissingEvidence {
                subject: "task closure",
                rule: "a waiver must cite every evidence reference the gate requires",
            });
        }
    }

    // Every waived gate needs one of those waivers; a bare `waived` state is an
    // assertion, not authority.
    for gate in &profile.definition.gates {
        if gate_states.get(&gate.id).copied() == Some(GateState::Waived)
            && !by_gate.contains_key(&gate.id)
        {
            return Err(DomainError::MissingAuthority {
                subject: "task closure",
                rule: "a waived gate must name the authority that waived it",
            });
        }
    }

    profile.certify_closure(completed_phases, gate_states, produced_artifacts)
}

/// Parse and validate a profile pack from its data form.
///
/// This is the loader for *any* pack; the bundled seeds go through it unchanged.
///
/// # Errors
/// Returns [`DomainError`] when the text is not a valid pack document or the
/// pack does not validate.
pub fn parse_pack(json: &str) -> DomainResult<ProfilePackSpec> {
    let pack: ProfilePackSpec = serde_json::from_str(json).map_err(|_| {
        DomainError::invalid("ProfilePackSpec", "is not a valid profile pack document")
    })?;
    validate_pack(&pack)?;
    Ok(pack)
}

/// Parse a pack whose team templates live in a separate data file.
///
/// # Errors
/// As [`parse_pack`].
pub fn parse_pack_with_teams(
    json: &str,
    teams: Vec<TeamTemplateSpec>,
) -> DomainResult<ProfilePackSpec> {
    let mut pack: ProfilePackSpec = serde_json::from_str(json).map_err(|_| {
        DomainError::invalid("ProfilePackSpec", "is not a valid profile pack document")
    })?;
    if !pack.teams.is_empty() {
        return Err(DomainError::invalid(
            "ProfilePackSpec",
            "already carries team templates of its own",
        ));
    }
    pack.teams = teams;
    validate_pack(&pack)?;
    Ok(pack)
}
