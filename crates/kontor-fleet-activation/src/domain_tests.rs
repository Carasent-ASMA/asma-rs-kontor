//! Domain artifact identity, guards and compatibility at the shared reader.

use super::*;
use crate::domain::{DOMAIN_GUARD, domain_path, read_domain, validate_domain};
use kontor_profiles::OperationalDomainPack;
use kontor_teams::CoreTeamRevision;

const SOURCE: &str = "packs/operational-domain.yml";
const YAML: &str = include_str!("../../../config/examples/packs/operational-domain.yml");

fn pin(bundle: &mut Bundle, text: &str) {
    let hash = ContentHash::of(text.as_bytes());
    bundle.manifest.sources.insert(SOURCE.into(), hash.clone());
    bundle.manifest.operational_domain = Some(OperationalDomainPin {
        source: SOURCE.into(),
        source_hash: hash,
    });
}

fn fixture() -> (Bundle, OperationalDomainPack) {
    let value: serde_json::Value = serde_yaml_ng::from_str(YAML).unwrap();
    let domain = kontor_profiles::pack::parse_operational_domain_pack(&value.to_string()).unwrap();
    let catalog = &domain.role_catalogs[0];
    let revision = CoreTeamRevision::resolve(
        SpecVersion::FIRST,
        catalog,
        &[],
        domain.delivery.purpose_roles(),
    )
    .unwrap();
    let mut bundle = published_bundle();
    bundle.roster = revision.canonicalize().unwrap();
    bundle.manifest.core_team_revision_hash = bundle.roster.hash().clone();
    bundle.manifest.role_catalog = RoleCatalogPin {
        catalog_id: catalog.catalog_id,
        version: catalog.version,
        content_hash: catalog.canonicalize().unwrap().hash().clone(),
    };
    pin(&mut bundle, YAML);
    (bundle, domain)
}

#[test]
fn legacy_manifest_bytes_stay_identical_and_do_not_supply_domain_authority() {
    let bundle = published_bundle();
    let document = bundle.manifest.canonicalize().unwrap();
    assert!(!document.json().contains("operational_domain"));
    let decoded: BundleManifest = document.deserialize().unwrap();
    assert_eq!(decoded.canonicalize().unwrap(), document);
    verify_bundle(bundle.path(), &bundle.bundle_hash).unwrap();
    assert_eq!(
        refused(read_domain(bundle.path(), &bundle.manifest, &bundle.roster)),
        "operational_domain_unpinned"
    );
}

#[test]
fn descriptor_requires_contained_yaml_reference_and_exact_source_map_entry() {
    let (mut bundle, _) = fixture();
    for source in [
        "",
        "/domain.yml",
        "../domain.yml",
        "packs/../domain.yml",
        "packs//domain.yml",
        "packs/domain.json",
        "packs/*.yml",
    ] {
        bundle.manifest.operational_domain.as_mut().unwrap().source = source.into();
        assert_eq!(
            refused(bundle.manifest.canonicalize()),
            "operational_domain_source_invalid",
            "{source}"
        );
    }
    bundle.manifest.operational_domain.as_mut().unwrap().source = SOURCE.into();
    bundle.manifest.sources.remove(SOURCE);
    assert_eq!(
        refused(bundle.manifest.canonicalize()),
        "operational_domain_source_hash_mismatch"
    );
}

#[test]
fn artifact_bytes_guards_and_hashes_are_checked_on_every_read() {
    let (bundle, expected) = fixture();
    let path = domain_path(bundle.path(), &ContentHash::of(YAML.as_bytes()));
    assert_eq!(
        refused(read_domain(bundle.path(), &bundle.manifest, &bundle.roster)),
        "operational_domain_artifact_missing"
    );
    private(&path, YAML.as_bytes());
    assert_eq!(
        read_domain(bundle.path(), &bundle.manifest, &bundle.roster).unwrap(),
        expected
    );
    private(&path, format!("{YAML}\n# changed\n").as_bytes());
    assert_eq!(
        refused(read_domain(bundle.path(), &bundle.manifest, &bundle.roster)),
        "operational_domain_artifact_hash_mismatch"
    );
    private(&path, YAML.as_bytes());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(
        refused(read_domain(bundle.path(), &bundle.manifest, &bundle.roster)),
        DOMAIN_GUARD.writable
    );
    private(&path, &[0xff]);
    assert_eq!(
        refused(read_domain(bundle.path(), &bundle.manifest, &bundle.roster)),
        DOMAIN_GUARD.utf8
    );
    private(&path, &vec![b' '; DOMAIN_GUARD.limit as usize + 1]);
    assert_eq!(
        refused(read_domain(bundle.path(), &bundle.manifest, &bundle.roster)),
        DOMAIN_GUARD.oversized
    );
    std::fs::remove_file(&path).unwrap();
    let target = bundle.path().join("another.yml");
    private(&target, YAML.as_bytes());
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert_eq!(
        refused(read_domain(bundle.path(), &bundle.manifest, &bundle.roster)),
        DOMAIN_GUARD.symlink
    );
}

#[test]
fn exact_bytes_change_identity_and_full_cross_references_refuse() {
    let (mut bundle, domain) = fixture();
    validate_domain(&bundle.manifest, &bundle.roster, YAML).unwrap();
    let original = bundle.manifest.canonicalize().unwrap();
    let reformatted = format!("# same resolved domain\n{YAML}");
    pin(&mut bundle, &reformatted);
    assert_eq!(
        validate_domain(&bundle.manifest, &bundle.roster, &reformatted).unwrap(),
        domain
    );
    assert_ne!(
        bundle.manifest.canonicalize().unwrap().hash(),
        original.hash()
    );

    let mut cases: Vec<(serde_json::Value, &str)> = Vec::new();
    let value = serde_json::to_value(&domain).unwrap();
    let mut changed = value.clone();
    changed["delivery"]["role_bindings"]
        .as_array_mut()
        .unwrap()
        .push(value["delivery"]["role_bindings"][0].clone());
    cases.push((changed, "operational_domain_invalid"));
    let mut changed = value.clone();
    changed["role_prompts"] = serde_json::json!([{"role_code":"LSA","prompt":"synthetic"},{"role_code":"LSA","prompt":"duplicate"}]);
    cases.push((changed, "operational_domain_invalid"));
    let mut changed = value.clone();
    changed["delivery"]["advisor_kind"] = "MISSING".into();
    cases.push((changed, "operational_domain_invalid"));
    let mut changed = value.clone();
    changed["team_definitions"][0]["topology"]["canonical_hash"] =
        ContentHash::of(b"other").as_str().into();
    cases.push((changed, "operational_domain_invalid"));
    let mut changed = value.clone();
    changed["delivery"]["control_role_code"] = "SA".into();
    cases.push((changed, "operational_domain_purpose_mismatch"));
    for (changed, expected) in cases {
        let text = serde_yaml_ng::to_string(&changed).unwrap();
        pin(&mut bundle, &text);
        assert_eq!(
            refused(validate_domain(&bundle.manifest, &bundle.roster, &text)),
            expected
        );
    }
    pin(&mut bundle, YAML);
    bundle.manifest.role_catalog.content_hash = ContentHash::of(b"another catalog");
    assert_eq!(
        refused(validate_domain(&bundle.manifest, &bundle.roster, YAML)),
        "operational_domain_catalog_mismatch"
    );
    bundle.manifest.role_catalog.content_hash = domain.role_catalogs[0]
        .canonicalize()
        .unwrap()
        .hash()
        .clone();
    bundle.manifest.core_team_revision_hash = ContentHash::of(b"another roster");
    assert_eq!(
        refused(validate_domain(&bundle.manifest, &bundle.roster, YAML)),
        "operational_domain_roster_mismatch"
    );
}
