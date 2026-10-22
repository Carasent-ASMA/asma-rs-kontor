//! Offline contracts for the selected authoring folder (TEST-006/008/010).

#[path = "support/config.rs"]
mod support;

use kontor_daemon::config_folder::{ConfigFolder, ENTRY};
use kontor_daemon::{Daemon, DaemonConfig};

#[test]
fn selection_is_explicit_and_the_flag_wins_over_the_environment() {
    assert!(
        ConfigFolder::select(None, None)
            .unwrap_err()
            .to_string()
            .contains("KONTOR_CONFIG_DIR")
    );
    let root = tempfile::tempdir().unwrap();
    let folder = support::write_config_folder(root.path());
    let missing = root.path().join("missing environment selection");
    let selected = ConfigFolder::select(Some(&folder), Some(&missing)).unwrap();
    assert_eq!(selected.root(), folder.canonicalize().unwrap());
    assert!(selected.root().is_absolute());
    assert_eq!(selected.project.key, "EXM");
    assert_eq!(selected.project.forge.base_branch, "main");
    let branch = selected.project.branch_rules.as_ref().unwrap();
    assert_eq!(branch.project_key, "EXM");
    assert_eq!(branch.types[0].as_str(), "feat");
    assert_eq!(
        selected.project.forge.repositories,
        ["example-org/example-project"]
    );
    assert_eq!(
        ConfigFolder::select(None, Some(&folder)).unwrap().root(),
        selected.root()
    );
    assert!(ConfigFolder::select(Some(&missing), Some(&folder)).is_err());
}

#[test]
fn selected_branch_templates_refuse_invalid_or_cross_project_fields_by_source() {
    let root = tempfile::tempdir().unwrap();
    let folder = support::write_config_folder(root.path());
    let path = folder.join(ENTRY);
    let original: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for (field, value, expected) in [
        ("branch", serde_json::Value::Null, "project.branch"),
        ("types", serde_json::json!([]), "project.branch.types"),
        (
            "types",
            serde_json::json!(["fix", "fix"]),
            "project.branch.types",
        ),
        ("types", serde_json::json!(["land"]), "project.branch.types"),
        (
            "pattern",
            serde_json::json!("^(feat)/ASMA-<number>-<slug>$"),
            "project.branch.pattern",
        ),
        ("pattern", serde_json::json!(".*"), "project.branch.pattern"),
        (
            "commit_pattern",
            serde_json::json!("^ASMA-<number> <summary>$"),
            "project.branch.commit_pattern",
        ),
        (
            "commit_pattern",
            serde_json::Value::Null,
            "project.branch.commit_pattern",
        ),
        (
            "base_branch",
            serde_json::json!("../main"),
            "project.forge.base_branch",
        ),
    ] {
        let mut entry = original.clone();
        if field == "branch" {
            entry["project"].as_mapping_mut().unwrap().remove("branch");
        } else if field == "base_branch" {
            entry["project"]["forge"][field] = serde_yaml_ng::to_value(value).unwrap();
        } else {
            entry["project"]["branch"][field] = serde_yaml_ng::to_value(value).unwrap();
        }
        std::fs::write(&path, serde_yaml_ng::to_string(&entry).unwrap()).unwrap();
        let error = ConfigFolder::select(Some(&folder), None).unwrap_err();
        assert_eq!(error.path, path.canonicalize().unwrap());
        assert!(error.problem.contains(expected), "{error}");
        assert!(!error.to_string().contains("ASMA"), "{error}");
    }
}

#[test]
fn selected_forge_checks_refuse_missing_ambiguous_or_conflicting_names() {
    let root = tempfile::tempdir().unwrap();
    let folder = support::write_config_folder(root.path());
    let entry_path = folder.join(ENTRY);
    let mut entry: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&entry_path).unwrap()).unwrap();
    let project_id = entry["project"]["project_id"]
        .as_str()
        .map(kontor_core::id::ProjectId::parse)
        .transpose()
        .unwrap()
        .unwrap_or_else(kontor_core::id::ProjectId::generate);
    std::fs::create_dir_all(folder.join("forge")).unwrap();
    let cases = [
        (vec!["publication-branch-title"], None),
        (
            vec!["first/publication-identity", "second/publication-identity"],
            None,
        ),
        (vec!["publication-identity", "publication-identity"], None),
        (
            vec!["example/publication-identity"],
            Some("conflicting-check-canary"),
        ),
    ];
    for (index, (checks, override_name)) in cases.into_iter().enumerate() {
        entry["project"]["check_runs"] = serde_yaml_ng::to_value(checks).unwrap();
        std::fs::write(&entry_path, serde_yaml_ng::to_string(&entry).unwrap()).unwrap();
        let mut app = serde_json::json!({
            "schema_version": 1,
            "app_id": 1,
            "installation_id": 2,
            "private_key_path": root.path().join("absent-key.pem"),
            "project_id": project_id,
            "repositories": ["example-org/example-project"],
        });
        if let Some(name) = override_name {
            app["check_name"] = serde_json::json!(name);
        }
        let text = serde_yaml_ng::to_string(&app).unwrap();
        let document: kontor_daemon::github_publication::GithubPublicationConfig =
            serde_yaml_ng::from_str(&text).expect("structurally valid public App fixture");
        document.validate().unwrap();
        std::fs::write(folder.join("forge/github-app.yml"), text).unwrap();
        ConfigFolder::select(Some(&folder), None).expect("valid project/App binding fixture");
        let error = Daemon::start_configured(
            DaemonConfig::at(root.path().join(format!("state-{index}"))).with_config_dir(&folder),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("forge/github-app.yml"), "{error}");
        assert!(!error.contains("conflicting-check-canary"), "{error}");
        assert!(
            !error.contains("private key"),
            "check identity is validated before key access"
        );
    }
}

#[test]
fn selected_tracker_and_app_bindings_constrain_the_explicit_project() {
    let root = tempfile::tempdir().unwrap();
    let folder = support::write_config_folder(root.path());
    let entry: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(folder.join(ENTRY)).unwrap()).unwrap();
    let id = entry["project"]["project_id"].as_str().unwrap();
    let other = kontor_core::id::ProjectId::generate().to_string();
    for source in [
        "tracker/jira.yml",
        "tracker/connectors.yml",
        "forge/github-app.yml",
    ] {
        let path = folder.join(source);
        let original = std::fs::read(&path).ok();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for (binding, expected) in [
            (id, None),
            (other.as_str(), Some("conflicts")),
            ("invalid-id-canary", Some("invalid")),
        ] {
            let document = if source == "tracker/connectors.yml" {
                serde_json::json!({"projects": [{"project_id": binding}]})
            } else {
                serde_json::json!({"project_id": binding})
            };
            std::fs::write(&path, serde_yaml_ng::to_string(&document).unwrap()).unwrap();
            let result = ConfigFolder::select(Some(&folder), None);
            if let Some(reason) = expected {
                let error = result.unwrap_err().to_string();
                assert!(
                    error.contains(source)
                        && error.contains("project_id")
                        && error.contains(reason),
                    "{error}"
                );
                assert!(
                    !error.contains(&other) && !error.contains("invalid-id-canary"),
                    "{error}"
                );
            } else {
                result.unwrap();
            }
        }
        if let Some(bytes) = original {
            std::fs::write(&path, bytes).unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
        }
    }
}

#[test]
fn publication_policy_refuses_partial_or_invalid_identity_by_field_name() {
    let root = tempfile::tempdir().unwrap();
    let folder = support::write_config_folder(root.path());
    let path = folder.join(ENTRY);
    let mut original: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    original["project"]
        .as_mapping_mut()
        .unwrap()
        .remove("project_id");
    original["project"]
        .as_mapping_mut()
        .unwrap()
        .remove("publication");
    let id = kontor_core::id::ProjectId::generate();
    for (project_id, publication, field) in [
        (Some(serde_json::json!(id)), None, "project.publication"),
        (
            None,
            Some(serde_json::json!({
                "root_repository": "example-org/example-project",
                "module_repositories": {},
            })),
            "project.project_id",
        ),
        (
            Some(serde_json::json!("not-an-id")),
            Some(serde_json::json!({
                "root_repository": "example-org/example-project",
                "module_repositories": {},
            })),
            "project.project_id",
        ),
        (
            Some(serde_json::json!(id)),
            Some(serde_json::json!({"root_repository": "example-org/example-project"})),
            "project.publication.module_repositories",
        ),
        (
            Some(serde_json::json!(id)),
            Some(serde_json::json!({
                "root_repository": "other-org/unlisted",
                "module_repositories": {},
            })),
            "project.publication.root_repository",
        ),
        (
            Some(serde_json::json!(id)),
            Some(serde_json::json!({
                "root_repository": "example-org/example-project",
                "module_repositories": {"components/*": "example-org/example-project"},
            })),
            "project.publication.module_repositories",
        ),
        (
            Some(serde_json::json!(id)),
            Some(serde_json::json!({
                "root_repository": "example-org/example-project",
                "module_repositories": {"components/worker": "example-org/*"},
            })),
            "project.publication.module_repositories",
        ),
    ] {
        let mut entry = original.clone();
        if let Some(value) = project_id {
            entry["project"]["project_id"] = serde_yaml_ng::to_value(value).unwrap();
        }
        if let Some(value) = publication {
            entry["project"]["publication"] = serde_yaml_ng::to_value(value).unwrap();
        }
        std::fs::write(&path, serde_yaml_ng::to_string(&entry).unwrap()).unwrap();
        let error = ConfigFolder::select(Some(&folder), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(field), "{field}: {error}");
    }
}

#[test]
fn references_resolve_from_the_selected_folder_and_refuse_escape_or_json() {
    let root = tempfile::tempdir().unwrap();
    let folder = support::write_config_folder(root.path());
    let selected = ConfigFolder::select(Some(&folder), None).unwrap();
    assert_eq!(
        selected.reference("packs/team.yml").unwrap(),
        folder.join("packs/team.yml").canonicalize().unwrap()
    );
    for bad in [
        "../outside.yml",
        "/tmp/outside.yml",
        "packs/team.json",
        "missing.yml",
    ] {
        assert!(selected.reference(bad).is_err(), "{bad}");
    }
    #[cfg(unix)]
    {
        std::fs::write(root.path().join("outside.yml"), "schema_version: 1\n").unwrap();
        std::os::unix::fs::symlink(root.path().join("outside.yml"), folder.join("escape.yml"))
            .unwrap();
        assert!(selected.reference("escape.yml").is_err());
    }
}

#[test]
fn startup_names_missing_invalid_and_conflicting_sources_before_creating_state() {
    let root = tempfile::tempdir().unwrap();
    let folder = support::write_config_folder(root.path());
    let state = root.path().join("uncreated state");
    let start = || Daemon::start_configured(DaemonConfig::at(&state).with_config_dir(&folder));
    std::fs::write(folder.join("host.yml"), "runtime: [\nprivate-marker\n").unwrap();
    let error = start().unwrap_err().to_string();
    assert!(error.contains("host.yml"), "{error}");
    assert!(!error.contains("private-marker"));
    assert!(!state.exists());
    std::fs::remove_file(folder.join("host.yml")).unwrap();
    std::fs::remove_file(folder.join("packs/team.yml")).unwrap();
    assert!(start().unwrap_err().to_string().contains("packs/team.yml"));
    assert!(!state.exists());
}

#[test]
fn absent_selection_and_non_folder_inputs_are_named_refusals() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("uncreated state");
    let error = Daemon::start_configured(DaemonConfig::at(&state))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no config folder selected"), "{error}");
    assert!(!state.exists());
    let file = root.path().join("a file");
    std::fs::write(&file, "{}").unwrap();
    assert!(
        ConfigFolder::select(Some(&file), None)
            .unwrap_err()
            .to_string()
            .contains("not a directory")
    );
    assert!(
        ConfigFolder::select(Some(&state), None)
            .unwrap_err()
            .to_string()
            .contains("does not exist")
    );
}

#[test]
fn all_legacy_sources_refuse_start_even_when_the_selected_yaml_is_valid() {
    let root = tempfile::tempdir().unwrap();
    let folder = support::write_config_folder(root.path());
    let state = root.path().join("state");
    std::fs::create_dir_all(state.join("config")).unwrap();
    for name in [
        "jira.json",
        "config/github-app.json",
        "runtimes.json",
        "memory-cognee.json",
        "succession-summarizer.json",
        "fleet.yml",
        "supervision.yml",
        "attention.yml",
        "quota-signals.yml",
    ] {
        let legacy = state.join(name);
        std::fs::write(&legacy, "{}").unwrap();
        let error = Daemon::start_configured(DaemonConfig::at(&state).with_config_dir(&folder))
            .unwrap_err()
            .to_string();
        assert!(error.contains(name) && error.contains("migrate"), "{error}");
        assert!(!state.join("kontor.db").exists());
        std::fs::remove_file(legacy).unwrap();
    }
    std::fs::write(folder.join("obsolete.json"), "{}").unwrap();
    assert!(
        ConfigFolder::select(Some(&folder), None)
            .unwrap_err()
            .to_string()
            .contains("obsolete.json")
    );
}

#[tokio::test]
async fn two_selected_folders_start_independent_realms_and_report_their_own_identity() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let folder_a = support::write_config_folder(a.path());
    let folder_b = support::write_config_folder(b.path());
    let entry = folder_b.join(ENTRY);
    let changed = std::fs::read_to_string(&entry)
        .unwrap()
        .replace("EXM", "ALT")
        .replace(
            "example-org/example-project",
            "alternate-org/alternate-project",
        )
        .replace("base_branch: main", "base_branch: develop");
    std::fs::write(&entry, changed).unwrap();
    let project_a = kontor_core::id::ProjectId::generate();
    let project_b = kontor_core::id::ProjectId::generate();
    for (folder, project, repository) in [
        (&folder_a, project_a, "example-org/example-project"),
        (&folder_b, project_b, "alternate-org/alternate-project"),
    ] {
        let path = folder.join(ENTRY);
        let mut value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        value["project"]["project_id"] = serde_yaml_ng::to_value(project).unwrap();
        value["project"]["publication"] = serde_yaml_ng::to_value(serde_json::json!({
            "root_repository": repository, "module_repositories": {},
        }))
        .unwrap();
        std::fs::write(path, serde_yaml_ng::to_string(&value).unwrap()).unwrap();
    }
    support::write_runtime_planes(
        &folder_a,
        &serde_json::json!({"schema_version":5,"runtimes":[]}),
    );
    let first = Daemon::start_configured(
        DaemonConfig::at(a.path().join("state"))
            .with_config_dir(&folder_a)
            .with_port(0),
    )
    .unwrap();
    let second = Daemon::start_configured(
        DaemonConfig::at(b.path().join("state"))
            .with_config_dir(&folder_b)
            .with_port(0),
    )
    .unwrap();
    let first_settings = first.config().config_folder().unwrap();
    let second_settings = second.config().config_folder().unwrap();
    assert_eq!(first_settings.project.key, "EXM");
    assert_eq!(
        first_settings
            .project
            .publication_policy
            .as_ref()
            .unwrap()
            .project_id,
        project_a
    );
    assert_eq!(second_settings.project.key, "ALT");
    assert_eq!(
        second_settings
            .project
            .publication_policy
            .as_ref()
            .unwrap()
            .project_id,
        project_b
    );
    assert_eq!(second_settings.project.forge.base_branch, "develop");
    assert_eq!(
        second_settings.project.forge.repositories,
        ["alternate-org/alternate-project"]
    );
    assert!(
        first
            .state()
            .with_store(|store| store.list_projects())
            .unwrap()
            .is_empty()
    );
    assert!(
        second
            .state()
            .with_store(|store| store.list_projects())
            .unwrap()
            .is_empty()
    );
    assert_ne!(first.realm_id(), second.realm_id());
    assert_ne!(first_settings.root(), second_settings.root());
    assert!(first.state().runtimes().families().next().is_none());
    first.shutdown();
    second.shutdown();
}

#[tokio::test]
async fn oversized_yaml_memory_settings_preserve_the_legacy_refusal() {
    let root = tempfile::tempdir().unwrap();
    let folder = support::write_config_folder(root.path());
    std::fs::write(
        folder.join("orchestration/memory.yml"),
        format!("#{}\nenabled: false\n", "padding".repeat(1400)),
    )
    .unwrap();
    let error = Daemon::start_configured(
        DaemonConfig::at(root.path().join("state")).with_config_dir(&folder),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("orchestration/memory.yml"), "{error}");
}
