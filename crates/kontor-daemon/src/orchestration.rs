//! The project orchestration authoring bundle, resolved into the exact
//! artifacts an aligned activation names (ASMA-8280 B-2).
//!
//! `config/orchestration/orchestration.yml` selects `fleet.yml` and
//! `teams/core-team.yml`. The Core Team source is the explicit selected roster:
//! it pins one role catalog by identity, revision and content hash and declares
//! every seat in order with its stable slot. The publisher here resolves that
//! source through the existing [`CoreTeamRevision::resolve`] contract, so the
//! canonical revision bytes — never role codes or a local convention — define
//! the `core_team_revision_hash` every `leadership/<hash>/<slot>` binding names.
//!
//! Policy parsing and validation stay in [`kontor_fleet`]; the manifest and
//! the verified reader are [`kontor_fleet_activation`]'s. This module reads
//! the two authoring documents that select them and assembles the bundle. It
//! writes nothing: [`crate::fleet::FleetSource::publish_bundle`] publishes and
//! [`crate::fleet::FleetSource::activate_bundle`] selects.

use std::collections::BTreeMap;

use kontor_core::id::{
    CanonicalDocument, ContentHash, ExternalName, RoleCode, RoleSlotId, SpecVersion,
};
use kontor_core::spec::RoleCatalogRevision;
use kontor_fleet::{FleetError, FleetSnapshot};
use kontor_fleet_activation::{BundleManifest, MANIFEST_SCHEMA_VERSION, RoleCatalogPin};
use kontor_teams::{CoreTeamRevision, CoreTeamSeatSelection, EpicPresence};
use serde::{Deserialize, Serialize};

/// The bundle root's selector, relative to `config/orchestration/`.
pub const ORCHESTRATION_SOURCE: &str = "orchestration.yml";

/// The fleet policy source the selector names.
pub const FLEET_SOURCE: &str = "fleet.yml";

/// The Core Team source the selector names.
pub const CORE_TEAM_SOURCE: &str = "teams/core-team.yml";

/// The only authoring format of either source this build reads.
pub const SOURCE_SCHEMA_VERSION: u32 = 1;

/// The stable rule texts a refusal names. None echoes a configured value.
#[allow(
    missing_docs,
    reason = "each constant is documented by the verbatim text it holds"
)]
pub mod rule {
    pub const O01: &str = "orchestration.yml is not a schema_version 1 orchestration source";
    pub const O02: &str = "orchestration.yml must select fleet.yml and teams/core-team.yml";
    pub const O03: &str = "teams/core-team.yml is not a schema_version 1 Core Team source";
    pub const O04: &str = "teams/core-team.yml pins a role catalog this realm does not hold under that identity, revision and hash";
    pub const O05: &str = "teams/core-team.yml does not resolve under its pinned role catalog";
    pub const O06: &str = "teams/core-team.yml must declare every seat, the mandatory roles included, in order, each with the slot its role resolves to";
}

use rule::{O01, O02, O03, O04, O05, O06};

/// `orchestration.yml`: which sources make up the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationSource {
    /// The authoring format: [`SOURCE_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The fleet policy source: [`FLEET_SOURCE`].
    pub fleet: String,
    /// The Core Team source: [`CORE_TEAM_SOURCE`].
    pub core_team: String,
}

/// `teams/core-team.yml`: the explicit selected roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreTeamSource {
    /// The authoring format: [`SOURCE_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The Core Team revision this source resolves to.
    pub version: SpecVersion,
    /// The one role catalog every seat resolves against.
    pub role_catalog: RoleCatalogPin,
    /// Every seat, in the order the revision records them.
    pub seats: Vec<CoreTeamSourceSeat>,
}

/// One declared seat of the selected roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreTeamSourceSeat {
    /// The stable slot this seat occupies; it must be the one its role
    /// resolves to, so a leadership binding can be written from the source.
    pub role_slot_id: RoleSlotId,
    /// The role code in the pinned catalog.
    pub role_code: RoleCode,
    /// Optional presentation-only label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_display_name: Option<ExternalName>,
    /// Epic materialization policy.
    pub presence: EpicPresence,
    /// Whether this role may start a Quick session.
    pub ad_hoc_allowed: bool,
}

/// The exact bytes of the three authoring sources.
#[derive(Debug, Clone, Copy)]
pub struct BundleSources<'a> {
    /// `orchestration.yml`.
    pub orchestration: &'a str,
    /// `fleet.yml`.
    pub fleet: &'a str,
    /// `teams/core-team.yml`.
    pub core_team: &'a str,
}

/// One authoring bundle resolved into the artifacts publication writes.
#[derive(Debug)]
pub struct ResolvedBundle {
    /// The policy, validated as a publishable policy.
    pub policy: FleetSnapshot,
    /// The resolved Core Team revision.
    pub revision: CoreTeamRevision,
    /// Its canonical document: the bytes and hash leadership bindings name.
    pub roster: CanonicalDocument,
    /// The manifest recording what was read and what was produced.
    pub manifest: BundleManifest,
}

/// Resolve one authoring bundle against the catalog its Core Team source pins.
///
/// `catalog` is the realm's revision under the pinned identity and revision;
/// its canonical hash must be the pinned hash. The declared seats must be the
/// resolved seats exactly, in order: the mandatory roles are declared, not
/// inserted, so the source is the whole roster a reviewer approved.
///
/// # Errors
/// O-01..O-06 for the selector and the Core Team source, and the policy's
/// own first refusal.
pub fn resolve_bundle(
    sources: &BundleSources<'_>,
    catalog: &RoleCatalogRevision,
) -> Result<ResolvedBundle, FleetError> {
    let selector: OrchestrationSource =
        serde_yaml_ng::from_str(sources.orchestration).map_err(|_| invalid(O01))?;
    if selector.schema_version != SOURCE_SCHEMA_VERSION {
        return Err(invalid(O01));
    }
    if selector.fleet != FLEET_SOURCE || selector.core_team != CORE_TEAM_SOURCE {
        return Err(invalid(O02));
    }
    let policy = FleetSnapshot::parse_policy(sources.fleet)?;
    let source: CoreTeamSource =
        serde_yaml_ng::from_str(sources.core_team).map_err(|_| invalid(O03))?;
    if source.schema_version != SOURCE_SCHEMA_VERSION {
        return Err(invalid(O03));
    }
    let pin = catalog_pin(catalog)?;
    if pin != source.role_catalog {
        return Err(invalid(O04));
    }
    let selections: Vec<CoreTeamSeatSelection> = source
        .seats
        .iter()
        .map(|seat| CoreTeamSeatSelection {
            role_code: seat.role_code.clone(),
            custom_display_name: seat.custom_display_name.clone(),
            presence: seat.presence,
            ad_hoc_allowed: seat.ad_hoc_allowed,
        })
        .collect();
    let revision = CoreTeamRevision::resolve(source.version, catalog, &selections)
        .map_err(|_| invalid(O05))?;
    let declared = source
        .seats
        .iter()
        .map(|seat| (&seat.role_slot_id, &seat.role_code));
    let resolved = revision
        .seats
        .iter()
        .map(|seat| (&seat.role_slot_id, &seat.role.role_code));
    if !declared.eq(resolved) {
        return Err(invalid(O06));
    }
    let roster = revision.canonicalize().map_err(|_| invalid(O05))?;
    let manifest = BundleManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        resolver: concat!("kontor ", env!("CARGO_PKG_VERSION")).to_owned(),
        sources: BTreeMap::from([
            (
                ORCHESTRATION_SOURCE.to_owned(),
                ContentHash::of(sources.orchestration.as_bytes()),
            ),
            (FLEET_SOURCE.to_owned(), policy.hash().clone()),
            (
                CORE_TEAM_SOURCE.to_owned(),
                ContentHash::of(sources.core_team.as_bytes()),
            ),
        ]),
        policy_hash: policy.hash().clone(),
        policy_schema_version: policy.schema_version(),
        role_catalog: pin,
        core_team_revision_hash: roster.hash().clone(),
    };
    Ok(ResolvedBundle {
        policy,
        revision,
        roster,
        manifest,
    })
}

/// The explicit authoring generator: an initial `teams/core-team.yml`
/// proposal holding exactly the mandatory roles, resolved against `catalog`.
///
/// The proposal is text for review, commit, publication and activation. It
/// is never a runtime fallback: nothing reads it until it is published as a
/// bundle and an activation names that bundle. A catalog update changes the
/// proposal and its hash, and needs a new explicit activation.
///
/// # Errors
/// O-05 when the catalog cannot seat the mandatory roles.
pub fn propose_core_team(catalog: &RoleCatalogRevision) -> Result<String, FleetError> {
    let revision =
        CoreTeamRevision::resolve(SpecVersion::FIRST, catalog, &[]).map_err(|_| invalid(O05))?;
    let source = CoreTeamSource {
        schema_version: SOURCE_SCHEMA_VERSION,
        version: revision.version,
        role_catalog: catalog_pin(catalog)?,
        seats: revision
            .seats
            .iter()
            .map(|seat| CoreTeamSourceSeat {
                role_slot_id: seat.role_slot_id.clone(),
                role_code: seat.role.role_code.clone(),
                custom_display_name: seat.role.custom_display_name.clone(),
                presence: seat.presence,
                ad_hoc_allowed: seat.ad_hoc_allowed,
            })
            .collect(),
    };
    serde_yaml_ng::to_string(&source).map_err(|_| invalid(O03))
}

/// The one supported `orchestration.yml`, for the same proposal.
///
/// # Errors
/// O-01 when it cannot be written as YAML.
pub fn propose_orchestration() -> Result<String, FleetError> {
    serde_yaml_ng::to_string(&OrchestrationSource {
        schema_version: SOURCE_SCHEMA_VERSION,
        fleet: FLEET_SOURCE.to_owned(),
        core_team: CORE_TEAM_SOURCE.to_owned(),
    })
    .map_err(|_| invalid(O01))
}

fn catalog_pin(catalog: &RoleCatalogRevision) -> Result<RoleCatalogPin, FleetError> {
    Ok(RoleCatalogPin {
        catalog_id: catalog.catalog_id,
        version: catalog.version,
        content_hash: catalog
            .canonicalize()
            .map_err(|_| invalid(O04))?
            .hash()
            .clone(),
    })
}

fn invalid(rule: &'static str) -> FleetError {
    FleetError::Invalid { rule }
}

/// One complete authoring bundle for the bundled catalog's mandatory roster,
/// binding every seat class, for the tests that publish and activate it.
#[cfg(test)]
pub(crate) mod fixture {
    use super::{propose_core_team, propose_orchestration};
    use kontor_core::spec::RoleCatalogRevision;
    use kontor_teams::CoreTeamRevision;

    pub(crate) const DELIVERY: &str = "team/01936f5a-0000-7000-8000-000000000102/implement";
    pub(crate) const REVIEWER: &str = "committee/01991c00-0000-7000-8000-000000000001/reviewer-a";
    pub(crate) const ADVISOR: &str = "advisor/01a02d00-0000-7000-8000-00000000ad01";

    pub(crate) struct Authoring {
        pub(crate) catalog: RoleCatalogRevision,
        pub(crate) orchestration: String,
        pub(crate) fleet: String,
        pub(crate) core_team: String,
    }

    impl Authoring {
        pub(crate) fn sources(&self) -> super::BundleSources<'_> {
            super::BundleSources {
                orchestration: &self.orchestration,
                fleet: &self.fleet,
                core_team: &self.core_team,
            }
        }
    }

    pub(crate) fn catalog() -> RoleCatalogRevision {
        kontor_profiles::seeds::bundled_operational_domain()
            .expect("the bundled domain loads")
            .role_catalogs
            .remove(0)
    }

    /// The v2 policy binding both leadership slots of `roster_hash`, one
    /// delivery, one committee and one advisor key.
    pub(crate) fn policy(roster_hash: &str) -> String {
        format!(
            "\
schema_version: 2
domains:
  codex: {{ provider: codex, accounts: [codex-work] }}
  claude: {{ provider: claude, accounts: [claude-personal] }}
models:
  sol: {{ domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }}
  opus: {{ domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }}
chains:
  codex-first:
    - [sol@xhigh]
    - [opus@xhigh]
  claude-first:
    - [opus@xhigh]
    - [sol@xhigh]
bindings:
  leadership/{roster_hash}/lsa: codex-first
  leadership/{roster_hash}/tpm: claude-first
  {DELIVERY}: claude-first
  {REVIEWER}: codex-first
  {ADVISOR}: claude-first
"
        )
    }

    pub(crate) fn authoring() -> Authoring {
        let catalog = catalog();
        let roster = CoreTeamRevision::resolve(kontor_core::id::SpecVersion::FIRST, &catalog, &[])
            .expect("LSA and TPM")
            .canonicalize()
            .expect("canonical roster");
        Authoring {
            orchestration: propose_orchestration().expect("selector"),
            fleet: policy(roster.hash().as_str()),
            core_team: propose_core_team(&catalog).expect("proposal"),
            catalog,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::authoring;
    use super::rule::*;
    use super::*;

    fn refused<T: std::fmt::Debug>(result: Result<T, FleetError>) -> &'static str {
        match result {
            Err(FleetError::Invalid { rule }) => rule,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn edited(yaml: &str, edit: impl FnOnce(&mut serde_yaml_ng::Value)) -> String {
        let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).expect("YAML");
        edit(&mut value);
        serde_yaml_ng::to_string(&value).expect("YAML")
    }

    #[test]
    fn the_explicit_proposal_resolves_to_the_exact_mandatory_revision() {
        let authoring = authoring();
        let bundle = resolve_bundle(&authoring.sources(), &authoring.catalog).expect("resolves");
        let expected = CoreTeamRevision::resolve(SpecVersion::FIRST, &authoring.catalog, &[])
            .expect("LSA and TPM");
        assert_eq!(bundle.revision, expected);
        assert_eq!(bundle.roster, expected.canonicalize().expect("canonical"));
        assert_eq!(
            bundle
                .revision
                .seats
                .iter()
                .map(|seat| seat.role_slot_id.as_str())
                .collect::<Vec<_>>(),
            ["lsa", "tpm"]
        );

        let manifest = &bundle.manifest;
        assert_eq!(manifest.core_team_revision_hash, *bundle.roster.hash());
        assert_eq!(
            manifest.policy_hash,
            ContentHash::of(authoring.fleet.as_bytes())
        );
        assert_eq!(manifest.policy_schema_version, 2);
        assert_eq!(
            manifest.role_catalog.content_hash,
            *authoring.catalog.canonicalize().expect("catalog").hash()
        );
        assert_eq!(
            bundle.revision.catalog_hash,
            manifest.role_catalog.content_hash
        );
        assert_eq!(
            manifest.sources,
            BTreeMap::from([
                (
                    ORCHESTRATION_SOURCE.to_owned(),
                    ContentHash::of(authoring.orchestration.as_bytes())
                ),
                (
                    FLEET_SOURCE.to_owned(),
                    ContentHash::of(authoring.fleet.as_bytes())
                ),
                (
                    CORE_TEAM_SOURCE.to_owned(),
                    ContentHash::of(authoring.core_team.as_bytes())
                ),
            ])
        );
        // Deterministic: the same bytes are the same bundle.
        let again = resolve_bundle(&authoring.sources(), &authoring.catalog).expect("resolves");
        assert_eq!(
            again.manifest.canonicalize().expect("canonical").hash(),
            manifest.canonicalize().expect("canonical").hash()
        );
    }

    #[test]
    fn the_roster_must_be_declared_whole_with_its_resolved_slots() {
        let authoring = authoring();
        let seats = |edit: fn(&mut Vec<serde_yaml_ng::Value>)| {
            edited(&authoring.core_team, |value| {
                let seats = value["seats"].as_sequence_mut().expect("seats");
                edit(seats);
            })
        };
        let cases = [
            // A mandatory role left for the resolver to insert.
            seats(|seats| {
                seats.remove(0);
            }),
            // A slot that belongs to another declared role.
            seats(|seats| {
                seats[0]["role_slot_id"] = serde_yaml_ng::Value::from("tpm");
            }),
            // A slot that is not the one its role resolves to.
            seats(|seats| {
                seats[0]["role_slot_id"] = serde_yaml_ng::Value::from("lead");
            }),
        ];
        for core_team in cases {
            let sources = BundleSources {
                core_team: &core_team,
                ..authoring.sources()
            };
            assert_eq!(
                refused(resolve_bundle(&sources, &authoring.catalog)),
                O06,
                "{core_team}"
            );
        }
    }

    #[test]
    fn a_source_the_publisher_cannot_prove_is_refused() {
        let authoring = authoring();
        let with = |orchestration: &str, core_team: &str| {
            refused(resolve_bundle(
                &BundleSources {
                    orchestration,
                    core_team,
                    ..authoring.sources()
                },
                &authoring.catalog,
            ))
        };
        let selector = |edit: fn(&mut serde_yaml_ng::Value)| edited(&authoring.orchestration, edit);
        let roster = |edit: fn(&mut serde_yaml_ng::Value)| edited(&authoring.core_team, edit);

        assert_eq!(
            with(
                &selector(|v| v["schema_version"] = 2.into()),
                &authoring.core_team
            ),
            O01
        );
        assert_eq!(
            with(
                &selector(|v| v["checkout"] = "main".into()),
                &authoring.core_team
            ),
            O01
        );
        assert_eq!(
            with(
                &selector(|v| v["fleet"] = "../fleet.yml".into()),
                &authoring.core_team
            ),
            O02
        );
        assert_eq!(
            with(
                &selector(|v| v["core_team"] = "teams/other.yml".into()),
                &authoring.core_team
            ),
            O02
        );
        assert_eq!(
            with(
                &authoring.orchestration,
                &roster(|v| v["schema_version"] = 2.into())
            ),
            O03
        );
        assert_eq!(
            with(
                &authoring.orchestration,
                &roster(|v| v["role_catalog"]["content_hash"] =
                    ContentHash::of(b"another catalog").as_str().into())
            ),
            O04
        );
        assert_eq!(
            with(
                &authoring.orchestration,
                &roster(|v| v["role_catalog"]["version"] = 2.into())
            ),
            O04
        );
        assert_eq!(
            with(
                &authoring.orchestration,
                &roster(|v| v["seats"][0]["presence"] = "on_demand".into())
            ),
            O05
        );

        // The policy keeps its own refusals.
        let sources = BundleSources {
            fleet: "schema_version: 3\n",
            ..authoring.sources()
        };
        assert!(resolve_bundle(&sources, &authoring.catalog).is_err());
    }

    #[test]
    fn every_rule_string_is_stable() {
        assert_eq!(
            O06,
            "teams/core-team.yml must declare every seat, the mandatory roles included, in order, each with the slot its role resolves to"
        );
        assert_eq!(
            O04,
            "teams/core-team.yml pins a role catalog this realm does not hold under that identity, revision and hash"
        );
    }
}
