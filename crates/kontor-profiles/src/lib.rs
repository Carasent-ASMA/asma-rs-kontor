//! `kontor-profiles` — Composable versioned work profiles, team templates and persona scenarios
//!
//! This crate composes the KON-MVP-03 documents — [`WorkProfileSpec`],
//! [`kontor_core::spec::PersonaScenarioSpec`] — and the typed team templates
//! from `kontor-teams` into validated, versioned **profile packs**, and resolves
//! one advertised category into an owned, content-hashed bundle a run can be
//! pinned to.
//!
//! It adds no repository, no CRUD service, no plugin registry and no trait
//! hierarchy: `kontor-store` already owns persistence and `kontor-core` already
//! owns the phase, gate and closure mechanics. What is genuinely missing at that
//! layer, and therefore lives here, is composition — the checks no single
//! document can make about itself:
//!
//! * does every pinned reference resolve, exactly once, at exactly that
//!   revision?
//! * is an artifact ever consumed before the phase that produces it can run?
//! * does the team a profile pinned actually seat the roles that profile hands
//!   work to, and carry the authority its gates demand?
//! * can a simulated persona reach authority over the gate it is testing,
//!   through the profile *or* through the team?
//!
//! ## Names are data
//!
//! Nothing in this crate compares an identifier to a literal. The ids the
//! bundled pack happens to use are fixture data in
//! `fixtures/mvp-profile-pack.json`; a deployment's own pack with entirely
//! different ids and a different graph shape takes the identical code path, and
//! the contract suite proves it by scanning this crate's source and by running
//! two unrelated packs through the same entry points.
//!
//! A consequence worth stating plainly, because it is the point of the design:
//! a profile that declares design, functionality-QA, design-QA and audit
//! obligations cannot be closed by satisfying its coding gate alone. That
//! follows from its own declared graph, not from anything recognizing what kind
//! of profile it is.

pub mod pack;
pub mod seeds;

pub use kontor_core::spec::WorkProfileSpec;
pub use pack::{
    ContextDefinition, GateWaiver, PackAvailability, PackCategoryKey, PackManifestEntry,
    PackPersona, ProfilePackKey, ProfilePackSpec, ResolvedProfileBundle, RoleDefinition,
    SkillDefinition, TaskTeamEvidence, certify_task_closure, parse_pack, parse_pack_with_teams,
    resolve_profile, revise_persona_scenario, revise_work_profile, validate_pack,
};
pub use seeds::bundled_pack;
