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

use crate::pack::{ProfilePackSpec, parse_pack_with_teams};

/// The profiles, manifest and personas this build ships, as data.
const BUNDLED_PACK: &str = include_str!("../fixtures/mvp-profile-pack.json");

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
