use std::path::{Path, PathBuf};
use std::process::Command;

use kontor_core::id::ModuleKey;
use kontor_core::pull_request::ForgeRepository;
use kontor_runtime::checkout::{CheckoutInputs, CheckoutSource, TaskCheckout};

pub(crate) const BASE: &str = "delivery-next";

pub(crate) fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}

pub(crate) fn repository(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root.join("components/worker")).unwrap();
    git(root, &["init", "-b", BASE]);
    git(root, &["config", "user.name", "Example Developer"]);
    git(root, &["config", "user.email", "developer@example.invalid"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    std::fs::write(root.join("components/worker/tracked.txt"), "base\n").unwrap();
    git(root, &["add", "components"]);
    git(root, &["commit", "-m", "fixture base"]);
    root.canonicalize().unwrap()
}

pub(crate) fn source(root: &Path) -> CheckoutSource {
    CheckoutSource {
        repository: ForgeRepository::parse("example-org/example-project").unwrap(),
        path: root.to_str().unwrap().to_owned(),
        base_ref: format!("refs/heads/{BASE}"),
    }
}

pub(crate) fn inputs(root: &Path) -> CheckoutInputs {
    CheckoutInputs {
        root: source(root),
        modules: Default::default(),
    }
}

pub(crate) fn snapshot(
    inputs: &CheckoutInputs,
    project: &Path,
    module: Option<&str>,
    destination: &Path,
) -> TaskCheckout {
    let module = module.map(|value| ModuleKey::parse(value).unwrap());
    TaskCheckout {
        schema_version: 1,
        source: inputs
            .select(project.to_str().unwrap(), module.as_ref(), BASE)
            .unwrap(),
        project_root: project.to_str().unwrap().to_owned(),
        claim: destination.to_str().unwrap().to_owned(),
        destination: destination.to_str().unwrap().to_owned(),
        branch: "feat/EXM-2-placed-task".to_owned(),
        confirmed_keys: vec!["EXM-2".to_owned()],
        module,
        managed_root: None,
    }
}
