//! Selected pack candidates; immutable installation and run pins stay with the store.

use std::collections::BTreeMap;

use kontor_core::id::ContentHash;
use kontor_profiles::pack::{ConsultationPresetPack, OperationalDomainPack, ProfilePackSpec};
use serde::Deserialize;

use crate::config_folder::{ConfigError, ConfigFolder};

/// Validated candidates from one folder, held unchanged for this process.
#[derive(Debug, Clone)]
pub struct RealmPack {
    /// Work profiles and their exact team revisions.
    pub profiles: ProfilePackSpec,
    /// Candidate topology, role catalogs and delivery correspondence.
    pub domain: OperationalDomainPack,
    /// Selected-folder-relative source retained in the bundle descriptor.
    pub domain_source: String,
    /// Exact captured bytes, shared by validation, preview and publication.
    pub domain_yaml: String,
    /// Committee revision candidates.
    pub presets: ConsultationPresetPack,
    /// Exact authored source hashes, by folder-relative YAML path.
    pub sources: BTreeMap<String, ContentHash>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    profile_pack: String,
    operational_domain: String,
    committee_presets: String,
}

/// Read and validate the selected authoring without installing or activating it.
///
/// # Errors
/// Names missing, escaping, malformed or inconsistent sources without their contents.
pub(crate) fn read(folder: &ConfigFolder, source: &str) -> Result<RealmPack, ConfigError> {
    let mut sources = BTreeMap::new();
    let mut document = |source: &str| {
        let text = folder.pack_source(source)?;
        let value: serde_json::Value = serde_yaml_ng::from_str(&text).map_err(|_| ConfigError {
            path: folder.root().join(source),
            problem: "invalid YAML pack source",
        })?;
        sources.insert(source.to_owned(), ContentHash::of(text.as_bytes()));
        Ok::<_, ConfigError>((value, text))
    };
    let invalid = |source: &str, problem| ConfigError {
        path: folder.root().join(source),
        problem,
    };
    let manifest: Manifest = serde_json::from_value(document(source)?.0)
        .map_err(|_| invalid(source, "invalid project.team_pack manifest"))?;
    if manifest.schema_version != 1 {
        return Err(invalid(
            source,
            "unsupported project.team_pack schema_version",
        ));
    }
    let profiles =
        kontor_profiles::pack::parse_pack(&document(&manifest.profile_pack)?.0.to_string())
            .map_err(|_| {
                invalid(
                    &manifest.profile_pack,
                    "invalid profile pack or team references",
                )
            })?;
    let (domain_value, domain_yaml) = document(&manifest.operational_domain)?;
    let domain = kontor_profiles::pack::parse_operational_domain_pack(&domain_value.to_string())
        .map_err(|error| {
            let problem = match error {
                kontor_core::DomainError::Invalid { subject, .. }
                    if matches!(
                        subject,
                        "delivery.architecture_closer_code"
                            | "delivery.process_closer_code"
                            | "delivery.control_role_code"
                    ) =>
                {
                    subject
                }
                _ => "invalid operational domain pack",
            };
            invalid(&manifest.operational_domain, problem)
        })?;
    let presets = kontor_profiles::pack::parse_consultation_presets(
        &document(&manifest.committee_presets)?.0.to_string(),
    )
    .map_err(|_| invalid(&manifest.committee_presets, "invalid committee preset pack"))?;
    for role in profiles
        .teams
        .iter()
        .flat_map(|team| &team.slots)
        .map(|slot| &slot.role.role)
        .chain(
            presets
                .committee_templates
                .iter()
                .flat_map(|template| &template.slots)
                .map(|slot| &slot.logical_role),
        )
    {
        if domain.delivery.role_code(role).is_none() {
            return Err(invalid(
                &manifest.operational_domain,
                "operational domain lacks a selected pack role binding",
            ));
        }
    }
    Ok(RealmPack {
        profiles,
        domain,
        domain_source: manifest.operational_domain,
        domain_yaml,
        presets,
        sources,
    })
}
