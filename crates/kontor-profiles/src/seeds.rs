//! The data boundary.
//!
//! Every behavioral name Kontor ships — profile ids, phase ids, gate ids, team
//! slot ids, persona vocabulary — lives in the two JSON files this module
//! embeds, and nowhere in Rust. The file is loaded through
//! [`crate::pack::parse_pack_with_teams`], which is the same loader a deployment
//! uses for a pack of its own, so the bundled data has no privileged path.
//!
//! There is deliberately no `match` and no comparison in this module. Adding one
//! would make the shipped names load-bearing, and the contract suite scans this
//! crate's source for exactly that.

use kontor_core::DomainResult;

use crate::pack::{
    ConsultationPresetPack, OperationalDomainPack, ProfilePackSpec, parse_consultation_presets,
    parse_operational_domain_pack, parse_pack_with_teams,
};

/// The profiles, manifest and personas this build ships, as data.
const BUNDLED_PACK: &str = include_str!("../fixtures/mvp-profile-pack.json");
/// The Operational topology and standard-role catalog this build ships.
const OPERATIONAL_DOMAIN: &str = include_str!("../fixtures/operational-domain.json");
/// The consultation presets this build ships.
const CONSULTATION_PRESETS: &str = include_str!("../fixtures/consultation-presets.json");

/// The pack bundled with this build.
///
/// Its team templates come from `kontor-teams`' own data file, so the two
/// slices each own their data and neither hard-codes the other's.
///
/// # Errors
/// Returns [`kontor_core::DomainError`] when the bundled data does not parse or
/// does not validate — which is a build-time defect in the data file, not a
/// runtime condition.
pub fn bundled_pack() -> DomainResult<ProfilePackSpec> {
    let teams = kontor_teams::spec::bundled_teams()?;
    parse_pack_with_teams(BUNDLED_PACK, teams.teams)
}

/// The bundled Operational topology and standard-role catalog.
///
/// # Errors
/// Returns [`kontor_core::DomainError`] when the checked-in data does not parse
/// or validate.
pub fn bundled_operational_domain() -> DomainResult<OperationalDomainPack> {
    parse_operational_domain_pack(OPERATIONAL_DOMAIN)
}

/// The consultation presets bundled with this build.
///
/// Publishing one is still an Admin apply against a project: this is the data a
/// deployment may publish, not a catalog that exists without anybody having
/// published it.
///
/// # Errors
/// Returns [`kontor_core::DomainError`] when the checked-in data does not parse
/// or validate.
pub fn bundled_consultation_presets() -> DomainResult<ConsultationPresetPack> {
    parse_consultation_presets(CONSULTATION_PRESETS)
}
