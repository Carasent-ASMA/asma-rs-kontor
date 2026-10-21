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
    assert_eq!(second_settings.project.key, "ALT");
    assert_eq!(second_settings.project.forge.base_branch, "develop");
    assert_eq!(
        second_settings.project.forge.repositories,
        ["alternate-org/alternate-project"]
    );
    assert_ne!(first.realm_id(), second.realm_id());
    assert_ne!(first_settings.root(), second_settings.root());
    assert!(first.state().runtimes().families().next().is_none());
    first.shutdown();
    second.shutdown();
}
