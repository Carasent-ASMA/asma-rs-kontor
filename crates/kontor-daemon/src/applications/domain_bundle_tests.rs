//! Exact selected domain bytes survive publication, activation and restart.

use super::config_fixture as config;
use super::*;
use crate::orchestration::{CoreTeamSource, CoreTeamSourceSeat, ResolvedBundle};
use kontor_fleet_activation::domain::{domain_path, read_domain};
use std::path::Path;

fn start(root: &Path, folder: &Path) -> crate::Daemon {
    crate::Daemon::start(
        crate::DaemonConfig::at(root)
            .with_config_dir(folder)
            .with_port(0),
        kontor_api::state::RuntimeRegistry::new(),
    )
    .unwrap()
}

fn resolve(services: &Services, folder: &Path) -> (ResolvedBundle, ContentHash, ContentHash) {
    let domain = services.purpose_source().unwrap();
    let catalog = &domain.role_catalogs[0];
    let roster = CoreTeamRevision::resolve(
        SpecVersion::FIRST,
        catalog,
        &[],
        domain.delivery.purpose_roles(),
    )
    .unwrap();
    let core_team = serde_yaml_ng::to_string(&CoreTeamSource {
        schema_version: 1,
        version: roster.version,
        role_catalog: kontor_fleet_activation::RoleCatalogPin {
            catalog_id: catalog.catalog_id,
            version: catalog.version,
            content_hash: roster.catalog_hash.clone(),
        },
        seats: roster
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
    })
    .unwrap();
    services
        .resolve_fleet_bundle(
            &std::fs::read_to_string(folder.join(crate::config_folder::ENTRY)).unwrap(),
            &std::fs::read_to_string(folder.join("orchestration/fleet.yml")).unwrap(),
            &core_team,
        )
        .unwrap()
}

#[test]
fn published_domain_is_exact_immutable_and_never_repaired_from_selected_yaml() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (mut bundle, hash, _) = resolve(&services, &folder);
    let original = bundle.operational_domain.clone().unwrap();
    let path = domain_path(
        &root,
        &bundle
            .manifest
            .operational_domain
            .as_ref()
            .unwrap()
            .source_hash,
    );
    let manifest_path = kontor_fleet_activation::manifest_path(&root, &hash);
    assert!(!path.exists());
    bundle.operational_domain = Some(format!("{original}\n# unpreviewed bytes"));
    assert!(
        services
            .fleet
            .publish_bundle(&bundle)
            .unwrap_err()
            .to_string()
            .contains("operational_domain_artifact_hash_mismatch")
    );
    assert!(!manifest_path.exists());
    assert!(!path.exists());
    bundle.operational_domain = Some(original.clone());
    let published = services.fleet.publish_bundle(&bundle).unwrap();
    assert_eq!(published.bundle_hash, hash);
    assert_eq!(std::fs::read(&path).unwrap(), original.as_bytes());
    assert!(!services.fleet.publish_bundle(&bundle).unwrap().created);
    assert!(
        !root
            .join(kontor_fleet_activation::FLEET_ACTIVATION_FILE)
            .exists()
    );
    services
        .fleet
        .activate_bundle(&hash, &crate::fleet::ActivationFence::default())
        .unwrap();
    let pointer_path = root.join(kontor_fleet_activation::FLEET_ACTIVATION_FILE);
    let pointer = std::fs::read(&pointer_path).unwrap();
    drop(services);
    drop(daemon);
    std::fs::write(
        folder.join("packs/operational-domain.yml"),
        format!("# unactivated source\n{original}"),
    )
    .unwrap();
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    assert_eq!(std::fs::read(&path).unwrap(), original.as_bytes());
    let verified = services.fleet.verify_bundle(&hash).unwrap();
    let first = read_domain(&root, &verified.manifest, &verified.roster).unwrap();
    assert!(first.role_prompts.is_empty());
    let dto = fleet_bundle_manifest_dto(&hash, &verified.manifest);
    assert_eq!(
        dto.operational_domain.unwrap().source,
        "packs/operational-domain.yml"
    );

    std::fs::write(&path, format!("# tampered\n{original}")).unwrap();
    assert!(
        services
            .fleet
            .verify_bundle(&hash)
            .unwrap_err()
            .to_string()
            .contains("operational_domain_artifact_hash_mismatch")
    );
    assert!(services.fleet.publish_bundle(&bundle).is_err());
    assert_ne!(std::fs::read(&path).unwrap(), original.as_bytes());
    assert!(
        services
            .fleet
            .activate_bundle(&hash, &crate::fleet::ActivationFence::default())
            .is_err()
    );
    assert_eq!(std::fs::read(&pointer_path).unwrap(), pointer);
    std::fs::remove_file(&path).unwrap();
    assert!(
        services
            .fleet
            .verify_bundle(&hash)
            .unwrap_err()
            .to_string()
            .contains("operational_domain_artifact_missing")
    );
    assert!(read_domain(&root, &verified.manifest, &verified.roster).is_err());
    assert!(
        services
            .fleet
            .publish_bundle(&bundle)
            .unwrap_err()
            .to_string()
            .contains("operational_domain_artifact_missing")
    );
    assert!(!path.exists());
    assert!(
        daemon
            .state()
            .with_store(|store| store.list_projects())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn domain_only_edits_change_bundle_and_preview_but_not_existing_activation() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let source = folder.join("packs/operational-domain.yml");
    let original = std::fs::read_to_string(&source).unwrap();
    let mut identities = std::collections::BTreeSet::new();
    let mut previews = std::collections::BTreeSet::new();
    let mut first = None;
    for change in ["original", "format", "prompt", "delivery"] {
        let mut value: serde_json::Value = serde_yaml_ng::from_str(&original).unwrap();
        let bytes = match change {
            "original" => original.clone(),
            "format" => format!("# formatting only\n{original}"),
            "prompt" => {
                value["role_prompts"] =
                    serde_json::json!([{"role_code":"LSA", "prompt":"Synthetic pinned prompt"}]);
                serde_yaml_ng::to_string(&value).unwrap()
            }
            "delivery" => {
                value["delivery"]["task_kind"] = "ASW".into();
                serde_yaml_ng::to_string(&value).unwrap()
            }
            _ => unreachable!(),
        };
        std::fs::write(&source, &bytes).unwrap();
        let daemon = start(&root, &folder);
        let services = daemon.jira_reconciler();
        let (bundle, hash, preview) = resolve(&services, &folder);
        assert_eq!(bundle.operational_domain.as_deref(), Some(bytes.as_str()));
        assert!(identities.insert(hash.clone()));
        assert!(previews.insert(preview));
        // The process uses the bytes captured for this candidate, not a live read.
        std::fs::write(&source, "invalid: [\n").unwrap();
        let (again, again_hash, _) = resolve(&services, &folder);
        assert_eq!(again_hash, hash);
        assert_eq!(again.operational_domain, bundle.operational_domain);
        let published = services.fleet.publish_bundle(&bundle).unwrap();
        let domain = read_domain(&root, &published.manifest, &bundle.roster).unwrap();
        if let Some((first_hash, first_domain, first_roster)) = &first {
            assert_eq!(
                kontor_fleet_activation::read_activation(&root)
                    .unwrap()
                    .source_bundle_hash
                    .as_ref(),
                Some(first_hash)
            );
            let pinned = services.fleet.verify_bundle(first_hash).unwrap();
            assert_eq!(
                &read_domain(&root, &pinned.manifest, &pinned.roster).unwrap(),
                first_domain
            );
            assert_eq!(&bundle.roster, first_roster);
        } else {
            services
                .fleet
                .activate_bundle(&hash, &crate::fleet::ActivationFence::default())
                .unwrap();
            first = Some((hash.clone(), domain.clone(), bundle.roster.clone()));
        }
        if change == "prompt" {
            assert_eq!(domain.role_prompts.len(), 1);
        }
        if change == "delivery" {
            assert_eq!(domain.delivery.task_kind.as_str(), "ASW");
        }
    }
}
