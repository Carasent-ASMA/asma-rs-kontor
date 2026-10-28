//! Selected YAML candidates do not replace immutable installed policy.
use super::*;
use kontor_core::repository::NewProject;
use std::path::Path;

use super::config_fixture as config;

fn write(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, serde_yaml_ng::to_string(value).unwrap()).unwrap();
}

fn read(path: &Path) -> serde_json::Value {
    serde_yaml_ng::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn start(root: &Path, folder: &Path) -> crate::Daemon {
    crate::Daemon::start(
        crate::DaemonConfig::at(root)
            .with_config_dir(folder)
            .with_port(0),
        kontor_api::state::RuntimeRegistry::new(),
    )
    .unwrap()
}

fn project(daemon: &crate::Daemon) -> ProjectId {
    let id = ProjectId::generate();
    daemon
        .state()
        .with_store(|store| {
            store.create_project(&NewProject {
                id,
                name: ExternalName::parse("Synthetic pack project").unwrap(),
                root_path: ExternalName::parse("/example/project").unwrap(),
                created_at: kontor_api::now(),
            })
        })
        .unwrap();
    id
}

#[test]
fn neutral_pack_is_complete_without_accounts_clinical_roles_or_project_prompt() {
    let root = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(root.path());
    let selected = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap();
    let pack = selected.realm_pack.unwrap();
    assert_eq!(pack.profiles.pack_id.as_str(), "kontor-default");
    assert_eq!(pack.sources.len(), 4);
    assert!(pack.profiles.personas.is_empty());
    assert!(pack.domain.role_prompts.is_empty());
    assert!(!pack.profiles.profiles.is_empty());
    assert!(!pack.presets.committee_templates.is_empty());
    assert!(pack.profiles.teams.iter().all(|team| {
        team.version.get() == 3 && team.slots.iter().all(|slot| slot.model_chain.is_none())
    }));
    for source in pack.sources.keys() {
        let text = std::fs::read_to_string(folder.join(source)).unwrap();
        let lowercase = text.to_lowercase();
        for private_marker in ["asma", "therapist", "clinical", "-personal", "-work"] {
            assert!(
                !lowercase.contains(private_marker),
                "neutral {source}: {private_marker}"
            );
        }
        assert_eq!(pack.sources[source], ContentHash::of(text.as_bytes()));
    }
}

#[test]
fn selected_pack_captures_exact_domain_bytes_and_refuses_unsafe_sources() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(root.path());
    let path = folder.join("packs/operational-domain.yml");
    let bytes = std::fs::read_to_string(&path).unwrap();
    let selected = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap();
    let pack = selected.realm_pack.as_ref().unwrap();
    assert_eq!(pack.domain_source, "packs/operational-domain.yml");
    assert_eq!(pack.domain_yaml, bytes);
    assert_eq!(
        pack.sources[&pack.domain_source],
        ContentHash::of(bytes.as_bytes())
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    let error = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
    assert!(error.path.ends_with("packs/operational-domain.yml"));
    assert_eq!(error.problem, "pack source is group or world writable");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(
        &path,
        format!(
            "# {}\n{bytes}",
            "x".repeat(kontor_fleet_activation::domain::MAX_DOMAIN_BYTES as usize)
        ),
    )
    .unwrap();
    let error = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
    assert!(error.path.ends_with("packs/operational-domain.yml"));
    assert_eq!(error.problem, "pack source exceeds the size limit");
    assert_eq!(pack.domain_yaml, bytes);
}

#[test]
fn two_realms_offer_their_own_cached_profiles_without_seeding_work_or_presets() {
    for name in ["alpha", "beta"] {
        let root = tempfile::tempdir().unwrap();
        let folder = config::write_config_folder(root.path());
        let path = folder.join("packs/profiles.yml");
        let mut profiles = read(&path);
        profiles["pack_id"] = name.into();
        profiles["manifest"][0]["label"] = format!("{name} work").into();
        write(&path, &profiles);
        let daemon = start(&root.path().join("state"), &folder);
        assert!(
            daemon
                .state()
                .with_store(|store| store.list_projects())
                .unwrap()
                .is_empty()
        );
        let apps = daemon.state();
        let offered = apps.applications().profile_packs().unwrap();
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].pack_id, name);
        assert_eq!(offered[0].source, "selected");
        let teams = apps.applications().team_templates().unwrap();
        assert!(!teams.is_empty());
        assert!(teams.iter().all(|team| team.source == "selected"));
        let before = apps.applications().work_profiles().unwrap();
        assert!(!before.is_empty());
        let at = kontor_api::now();
        let services = daemon.jira_reconciler();
        let (bundle, source) = services.bundle_with_team_source("code", at).unwrap();
        assert_eq!(source, TeamTemplateSource::Registered);
        // A running process never hot-reads candidate bytes.
        profiles["manifest"][0]["label"] = "an unactivated edit".into();
        write(&path, &profiles);
        assert_eq!(services.bundle("code", at).unwrap(), bundle);
        assert_eq!(
            apps.applications().work_profiles().unwrap()[0].label,
            before[0].label
        );
        let id = project(&daemon);
        assert!(
            apps.applications()
                .committee_templates(id)
                .unwrap()
                .revisions
                .is_empty()
        );
    }
}

#[test]
fn absent_selected_pack_has_no_compiled_profile_or_preset_fallback() {
    let root = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(root.path());
    let path = folder.join(crate::config_folder::ENTRY);
    let mut entry = read(&path);
    entry["project"]
        .as_object_mut()
        .unwrap()
        .remove("team_pack");
    write(&path, &entry);
    let daemon = start(&root.path().join("state"), &folder);
    let id = project(&daemon);
    let state = daemon.state();
    assert!(state.applications().profile_packs().unwrap().is_empty());
    assert!(state.applications().work_profiles().unwrap().is_empty());
    assert!(state.applications().team_templates().unwrap().is_empty());
    assert!(
        state
            .applications()
            .committee_templates(id)
            .unwrap()
            .revisions
            .is_empty()
    );
    assert!(state.applications().work_profile("feature").is_err());
}

#[test]
fn malformed_pack_sources_and_cross_references_refuse_by_selected_filename() {
    let root = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(root.path());
    let manifest_path = folder.join("packs/team.yml");
    let original = read(&manifest_path);
    for value in [
        serde_json::json!({"schema_version":2,"profile_pack":"packs/profiles.yml","operational_domain":"packs/operational-domain.yml","committee_presets":"packs/committee-presets.yml"}),
        serde_json::json!({"schema_version":1,"teams":[]}),
        serde_json::json!({"schema_version":1,"profile_pack":"packs/profiles.yml","operational_domain":"packs/operational-domain.yml","committee_presets":"packs/committee-presets.yml","purposes":{}}),
    ] {
        write(&manifest_path, &value);
        let error = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
        assert_eq!(error.path, manifest_path.canonicalize().unwrap());
        assert!(error.problem.contains("project.team_pack"));
    }
    for reference in [
        "packs/absent.yml",
        "../outside.yml",
        "/absolute.yml",
        "packs/profiles.json",
    ] {
        let mut value = original.clone();
        value["profile_pack"] = reference.into();
        write(&manifest_path, &value);
        assert!(crate::config_folder::ConfigFolder::select(Some(&folder), None).is_err());
    }
    write(&manifest_path, &original);
    for (source, field) in [
        ("packs/profiles.yml", "teams"),
        ("packs/committee-presets.yml", "committee_templates"),
    ] {
        let path = folder.join(source);
        let value = read(&path);
        let mut invalid = value.clone();
        invalid[field] = serde_json::json!([{}]);
        write(&path, &invalid);
        let error = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
        assert_eq!(error.path, path.canonicalize().unwrap());
        write(&path, &value);
    }
    let domain = folder.join("packs/operational-domain.yml");
    let mut value = read(&domain);
    value["delivery"]["role_bindings"] = serde_json::json!([]);
    write(&domain, &value);
    let error = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
    assert_eq!(error.path, domain.canonicalize().unwrap());
    assert!(error.problem.contains("role binding"));
}

#[tokio::test]
async fn committee_candidates_require_explicit_preview_apply_and_installed_bytes_survive_edits() {
    let root = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(root.path());
    let selected = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap();
    let definition =
        serde_json::to_value(&selected.realm_pack.unwrap().presets.committee_templates[0]).unwrap();
    let state_root = root.path().join("state");
    let daemon = start(&state_root, &folder);
    let id = project(&daemon);
    let state = daemon.state();
    let apps = state.applications();
    let catalog = apps.committee_templates(id).unwrap();
    assert!(catalog.revisions.is_empty());
    let preview = apps
        .preview_committee_template(
            id,
            &ProfilePreviewRequest {
                definition: definition.clone(),
            },
        )
        .unwrap();
    assert!(preview.violations.is_empty());
    let installed = apps
        .apply_committee_template(
            &IdempotencyKey::parse("publish-own-preset").unwrap(),
            id,
            &ProfileApplyRequest {
                definition: definition.clone(),
                preview_hash: preview.preview_hash,
                expected_revision: catalog.revision,
            },
        )
        .await
        .unwrap();
    let before = apps.committee_templates(id).unwrap();
    assert_eq!(before.revisions, vec![installed.published]);
    drop(state);
    drop(daemon);
    let path = folder.join("packs/committee-presets.yml");
    let mut presets = read(&path);
    presets["committee_templates"][0]["name"] = "unactivated rename".into();
    write(&path, &presets);
    let daemon = start(&state_root, &folder);
    let state = daemon.state();
    let after = state.applications().committee_templates(id).unwrap();
    assert_eq!(before.revisions, after.revisions);
    assert_eq!(before.revision, after.revision);
    let edited = presets["committee_templates"][0].clone();
    let preview = state
        .applications()
        .preview_committee_template(
            id,
            &ProfilePreviewRequest {
                definition: edited.clone(),
            },
        )
        .unwrap();
    assert!(
        state
            .applications()
            .apply_committee_template(
                &IdempotencyKey::parse("conflicting-preset").unwrap(),
                id,
                &ProfileApplyRequest {
                    definition: edited,
                    preview_hash: preview.preview_hash,
                    expected_revision: after.revision,
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        state
            .applications()
            .committee_templates(id)
            .unwrap()
            .revisions,
        before.revisions
    );
}

#[test]
#[ignore = "explicit read-only ASMA input conversion check; set KONTOR_PACK_EQUIVALENCE_FOLDER"]
fn asma_yaml_overlay_preserves_the_authored_pack_domain_and_committee_bytes() {
    let folder = std::env::var_os("KONTOR_PACK_EQUIVALENCE_FOLDER")
        .expect("explicit isolated ASMA config folder");
    let selected =
        crate::config_folder::ConfigFolder::select(Some(Path::new(&folder)), None).unwrap();
    let pack = selected.realm_pack.as_ref().unwrap();
    assert_eq!(
        pack.profiles,
        kontor_profiles::seeds::bundled_pack().unwrap()
    );
    assert_eq!(
        pack.domain,
        kontor_profiles::bundled_operational_domain().unwrap()
    );
    assert_eq!(
        pack.presets,
        kontor_profiles::seeds::bundled_consultation_presets().unwrap()
    );
    assert_eq!(selected.project.key, "ASMA");
    assert!(selected.project.publication_policy.is_none());
    assert_eq!(
        selected.project.check_runs,
        ["asma/publication-branch-title", "asma/publication-identity"]
    );
    let state_root = tempfile::tempdir().unwrap();
    let daemon = crate::Daemon::start_configured(
        crate::DaemonConfig::at(state_root.path())
            .with_config_dir(&folder)
            .with_port(0),
    )
    .unwrap();
    assert!(
        daemon
            .state()
            .with_store(|store| store.list_projects())
            .unwrap()
            .is_empty()
    );
    let connection = rusqlite::Connection::open_with_flags(
        state_root.path().join(crate::DATABASE_FILE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    for table in [
        "projects",
        "mini_projects",
        "tasks",
        "team_runs",
        "agent_runs",
        "epic_rosters",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "empty business state: {table}");
    }
    println!(
        "PACK_PROOF selected={} state={} projects=0 epics=0 tasks=0 team_runs=0 agent_runs=0 epic_rosters=0",
        selected.root().display(),
        state_root.path().display()
    );
}

#[test]
fn selected_domain_selector_refusals_preserve_the_field_name() {
    for key in ["architecture_closer_code", "control_role_code"] {
        for missing in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let folder = config::write_config_folder(root.path());
            let path = folder.join("packs/operational-domain.yml");
            let mut domain = read(&path);
            if missing {
                domain["delivery"].as_object_mut().unwrap().remove(key);
            } else {
                domain["delivery"][key] = "UNKNOWN".into();
            }
            write(&path, &domain);
            let error =
                crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
            assert_eq!(error.problem, format!("delivery.{key}"));
            assert!(error.path.ends_with("packs/operational-domain.yml"));
        }
    }
}

#[test]
fn selected_project_holiday_countries_are_explicit_and_have_no_default() {
    let root = tempfile::tempdir().unwrap();
    let folder = config::write_config_folder(root.path());
    let path = folder.join(crate::config_folder::ENTRY);
    let original = read(&path);
    let mut entry = original.clone();
    entry["project"]["holiday_countries"] = serde_json::json!(["DE"]);
    write(&path, &entry);
    let selected = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap();
    assert_eq!(
        selected
            .project
            .holiday_countries
            .iter()
            .map(|country| country.as_str())
            .collect::<Vec<_>>(),
        ["DE"]
    );
    for value in [
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!([false]),
        serde_json::json!(["unknown"]),
    ] {
        let mut entry = original.clone();
        if value.is_null() {
            entry["project"]
                .as_object_mut()
                .unwrap()
                .remove("holiday_countries");
        } else {
            entry["project"]["holiday_countries"] = value;
        }
        write(&path, &entry);
        let error = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap_err();
        assert!(error.problem.contains("project.holiday_countries"));
        assert!(error.path.ends_with(crate::config_folder::ENTRY));
    }
}
