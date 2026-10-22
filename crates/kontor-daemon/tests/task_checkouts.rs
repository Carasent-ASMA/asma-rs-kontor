//! Disposable Git proofs for selected host checkout sources; no network or live root.
#[path = "support/config.rs"]
mod config;
#[path = "support/checkouts.rs"]
mod fixture;

use fixture::{BASE, git, inputs, repository, snapshot, source};
use kontor_core::id::ModuleKey;
use kontor_daemon::config_folder::{ConfigFolder, ENTRY};
use kontor_runtime::checkout::{self, CheckoutError};
use std::path::Path;

fn temp() -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    (directory, root)
}

#[test]
fn exact_nested_module_overrides_monorepo_and_new_places_require_declarations() {
    let (_directory, root) = temp();
    let project = repository(&root.join("root"));
    let independent = repository(&root.join("independent"));
    let mut selected = inputs(&project);
    let module = ModuleKey::parse("components/worker").unwrap();
    assert_eq!(
        selected
            .select(project.to_str().unwrap(), Some(&module), BASE)
            .unwrap()
            .declaration
            .path,
        project.to_str().unwrap()
    );
    let missing = ModuleKey::parse("components/new-worker").unwrap();
    assert_eq!(
        selected
            .select(project.to_str().unwrap(), Some(&missing), BASE)
            .unwrap_err(),
        CheckoutError("host.checkouts.module_record_required")
    );
    selected
        .modules
        .insert(module.clone(), source(&independent));
    assert_eq!(
        selected
            .select(project.to_str().unwrap(), Some(&module), BASE)
            .unwrap()
            .declaration
            .path,
        independent.to_str().unwrap()
    );
    selected.modules.insert(missing.clone(), source(&project));
    assert!(
        selected
            .select(project.to_str().unwrap(), Some(&missing), BASE)
            .is_ok()
    );
    // A sibling/prefix never inherits the exact override.
    let sibling = ModuleKey::parse("components/worker-extra").unwrap();
    assert!(
        selected
            .select(project.to_str().unwrap(), Some(&sibling), BASE)
            .is_err()
    );
    assert_eq!(
        selected
            .select(independent.to_str().unwrap(), None, BASE)
            .unwrap_err(),
        CheckoutError("host.checkouts.root.project_root_mismatch")
    );
}

#[test]
fn configured_full_base_ref_is_resolved_once_without_head_or_remote_guesses() {
    let (_directory, root) = temp();
    let project = repository(&root.join("root"));
    let initial = git(&project, &["rev-parse", "HEAD"]);
    let selected = inputs(&project);
    let frozen = snapshot(
        &selected,
        &project,
        Some("components/worker"),
        &root.join("task"),
    );
    std::fs::write(project.join("later.txt"), "later").unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "-m", "advance explicit base"]);
    git(&project, &["checkout", "-b", "different-current-branch"]);
    checkout::prepare(&frozen).unwrap();
    assert_eq!(
        git(Path::new(&frozen.destination), &["rev-parse", "HEAD"]),
        initial
    );
    assert!(!Path::new(&frozen.destination).join("later.txt").exists());
    assert_eq!(
        checkout::readback(&frozen).unwrap()["common_dir"],
        frozen.source.common_dir
    );
    checkout::prepare(&frozen).unwrap();
    let mut declared = source(&project);
    for invalid in [
        "HEAD",
        "main",
        "refs/heads/main",
        "refs/remotes/origin/delivery-next",
        "refs/heads/delivery-next/../x",
    ] {
        declared.base_ref = invalid.to_owned();
        assert!(
            checkout::resolve_source(&declared, BASE).is_err(),
            "{invalid}"
        );
    }
    git(
        &project,
        &["remote", "add", "upstream", "https://example.invalid/root"],
    );
    git(
        &project,
        &[
            "update-ref",
            "refs/remotes/upstream/delivery-next",
            &initial,
        ],
    );
    declared.base_ref = "refs/remotes/upstream/delivery-next".to_owned();
    assert_eq!(
        checkout::resolve_source(&declared, BASE)
            .unwrap()
            .base_commit
            .as_str(),
        initial
    );
    let blob = git(&project, &["rev-parse", "HEAD:later.txt"]);
    git(
        &project,
        &["update-ref", "refs/remotes/upstream/delivery-next", &blob],
    );
    assert_eq!(
        checkout::resolve_source(&declared, BASE).unwrap_err(),
        CheckoutError("host.checkouts.base_ref_noncommit")
    );
}

#[test]
fn remote_task_branch_requires_explicit_input_and_never_creates_a_destination() {
    let (_directory, root) = temp();
    let project = repository(&root.join("root"));
    let frozen = snapshot(&inputs(&project), &project, None, &root.join("task"));
    git(
        &project,
        &[
            "update-ref",
            &format!("refs/remotes/custom/{}", frozen.branch),
            frozen.source.base_commit.as_str(),
        ],
    );
    assert_eq!(
        checkout::prepare(&frozen).unwrap_err(),
        CheckoutError("checkout.remote_task_branch_input_required")
    );
    assert!(!root.join("task").exists());
}

#[test]
fn source_and_destination_must_be_exact_checkout_roots_and_share_common_directory() {
    let (_directory, root) = temp();
    let project = repository(&root.join("root"));
    let foreign = repository(&root.join("foreign"));
    let mut declared = source(&project.join("components"));
    assert_eq!(
        checkout::resolve_source(&declared, BASE).unwrap_err(),
        CheckoutError("host.checkouts.path_is_not_checkout_root")
    );
    declared.path = root.join("missing").to_str().unwrap().to_owned();
    assert!(checkout::resolve_source(&declared, BASE).is_err());
    let frozen = snapshot(&inputs(&project), &project, None, &foreign);
    git(&foreign, &["checkout", "-b", &frozen.branch]);
    assert_eq!(
        checkout::verify(&frozen).unwrap_err(),
        CheckoutError("checkout.destination_foreign_common_dir")
    );
    let frozen = snapshot(&inputs(&project), &project, None, &root.join("task"));
    checkout::prepare(&frozen).unwrap();
    git(
        Path::new(&frozen.destination),
        &["checkout", "-b", "fix/EXM-2-other"],
    );
    assert_eq!(
        checkout::verify(&frozen).unwrap_err(),
        CheckoutError("checkout.destination_branch_changed")
    );
}

#[cfg(unix)]
#[test]
fn gitlinks_symlinks_and_nested_independent_repositories_are_not_monorepo_fallbacks() {
    let (_directory, root) = temp();
    let project = repository(&root.join("root"));
    let selected = inputs(&project);
    let module = ModuleKey::parse("components/worker").unwrap();
    let oid = git(&project, &["rev-parse", "HEAD"]);
    git(&project, &["rm", "-r", "components/worker"]);
    git(
        &project,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{oid},components/worker"),
        ],
    );
    git(&project, &["commit", "-m", "gitlink boundary"]);
    assert_eq!(
        selected
            .select(project.to_str().unwrap(), Some(&module), BASE)
            .unwrap_err(),
        CheckoutError("host.checkouts.module_repository_boundary")
    );
    git(&project, &["reset", "--hard", &oid]);
    std::fs::remove_dir_all(project.join("components/worker")).unwrap();
    std::os::unix::fs::symlink(root.join("elsewhere"), project.join("components/worker")).unwrap();
    assert_eq!(
        selected
            .select(project.to_str().unwrap(), Some(&module), BASE)
            .unwrap_err(),
        CheckoutError("host.checkouts.module_symlink_boundary")
    );
    std::fs::remove_file(project.join("components/worker")).unwrap();
    let independent = repository(&project.join("components/worker"));
    assert_eq!(
        selected
            .select(project.to_str().unwrap(), Some(&module), BASE)
            .unwrap_err(),
        CheckoutError("host.checkouts.module_repository_boundary")
    );
    let mut explicit = selected;
    explicit
        .modules
        .insert(module.clone(), source(&independent));
    assert!(
        explicit
            .select(project.to_str().unwrap(), Some(&module), BASE)
            .is_ok()
    );
    let mut frozen = snapshot(
        &explicit,
        &project,
        Some("components/worker"),
        &project.join("managed/task"),
    );
    frozen.managed_root = Some(project.join("managed").to_str().unwrap().to_owned());
    std::os::unix::fs::symlink(&root, project.join("managed")).unwrap();
    assert!(
        frozen.validate().is_err(),
        "a symlink cannot move a managed destination outside the root"
    );
}

fn selected_folder(root: &Path, checkout: &serde_json::Value) -> std::path::PathBuf {
    let folder = config::write_config_folder(root);
    let path = folder.join(ENTRY);
    let mut entry: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    entry["project"]["forge"]["base_branch"] = BASE.into();
    std::fs::write(path, serde_yaml_ng::to_string(&entry).unwrap()).unwrap();
    config::write_runtime_planes(
        &folder,
        &serde_json::json!({"schema_version":5,"runtimes":[]}),
    );
    let path = folder.join("host.yml");
    let mut host: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    host["checkouts"] = checkout.clone();
    std::fs::write(path, serde_yaml_ng::to_string(&host).unwrap()).unwrap();
    folder
}

#[test]
fn selected_host_validation_is_read_only_optional_and_names_invalid_fields() {
    let (_directory, root) = temp();
    let project = repository(&root.join("root"));
    let declarations = serde_json::json!({"root":source(&project), "modules":{}});
    let folder = selected_folder(&root, &declarations);
    let before = git(&project, &["show-ref"]);
    let selected = ConfigFolder::select(Some(&folder), None).unwrap();
    assert!(selected.checkouts.is_some());
    assert_eq!(before, git(&project, &["show-ref"]));
    for (field, value) in [
        ("repository", serde_json::json!("foreign/other")),
        (
            "path",
            serde_json::json!(project.join("components").to_str().unwrap()),
        ),
        ("base_ref", serde_json::json!("HEAD")),
    ] {
        let mut invalid = declarations.clone();
        invalid["root"][field] = value;
        selected_folder(&root, &invalid);
        let error = ConfigFolder::select(Some(&folder), None).unwrap_err();
        assert_eq!(error.path, folder.join("host.yml"));
        assert!(error.problem.contains(field), "{error:?}");
    }
    for invalid in [
        serde_json::json!({"root":source(&project)}),
        serde_json::json!({"root":source(&project),"modules":{"components/*":source(&project)}}),
        serde_json::json!({"root":{"path":project},"modules":{}}),
    ] {
        selected_folder(&root, &invalid);
        assert!(ConfigFolder::select(Some(&folder), None).is_err());
    }
    selected_folder(&root, &declarations);
    let host = folder.join("host.yml");
    std::fs::write(&host, format!("schema_version: 1\ncheckouts:\n  root:\n    repository: example-org/example-project\n    path: {}\n    base_ref: refs/heads/{BASE}\n  modules:\n    components/worker: {{}}\n    components/worker: {{}}\n",project.display())).unwrap();
    assert!(ConfigFolder::select(Some(&folder), None).is_err());
    config::write_runtime_planes(
        &folder,
        &serde_json::json!({"schema_version":5,"runtimes":[]}),
    );
    assert!(
        ConfigFolder::select(Some(&folder), None)
            .unwrap()
            .checkouts
            .is_none()
    );
}

#[test]
fn registered_submodule_source_requires_exact_module_record_and_has_its_own_common_dir() {
    let (_directory, root) = temp();
    let project = repository(&root.join("root"));
    let origin = repository(&root.join("independent"));
    git(
        &project,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            origin.to_str().unwrap(),
            "components/independent",
        ],
    );
    git(&project, &["commit", "-m", "registered submodule"]);
    let module = ModuleKey::parse("components/independent").unwrap();
    let mut selected = inputs(&project);
    assert_eq!(
        selected
            .select(project.to_str().unwrap(), Some(&module), BASE)
            .unwrap_err(),
        CheckoutError("host.checkouts.module_repository_boundary")
    );
    let submodule = project
        .join("components/independent")
        .canonicalize()
        .unwrap();
    selected.modules.insert(module, source(&submodule));
    let frozen = snapshot(
        &selected,
        &project,
        Some("components/independent"),
        &root.join("submodule-task"),
    );
    assert!(Path::new(&frozen.source.common_dir).starts_with(project.join(".git/modules")));
    checkout::prepare(&frozen).unwrap();
    checkout::verify(&frozen).unwrap();
    assert_ne!(
        frozen.source.common_dir,
        selected
            .select(project.to_str().unwrap(), None, BASE)
            .unwrap()
            .common_dir
    );
}
