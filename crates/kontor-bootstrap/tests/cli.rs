//! Binary-level qualification over synthetic roots: the real `kontor-bootstrap`
//! executable, assert_cmd, and a redaction check on everything it prints.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use kontor_bootstrap::artifact::manifest::{
    ArtifactEntry, ArtifactManifest, ArtifactName, MANIFEST_SCHEMA_VERSION, sha256_hex,
};
use serde_json::Value;

struct World {
    _holder: tempfile::TempDir,
    install_root: PathBuf,
    source_dir: PathBuf,
    manifest: PathBuf,
    home_root: PathBuf,
    state_root: PathBuf,
}

impl World {
    fn new() -> Self {
        let holder = tempfile::tempdir().expect("tempdir");
        let install_root = holder.path().join("install root");
        let source_dir = holder.path().join("release one");
        let home_root = holder.path().join("home root");
        let state_root = holder.path().join("state root");
        std::fs::create_dir_all(&source_dir).expect("source");
        std::fs::create_dir_all(&home_root).expect("home");
        for directory in [".codex", ".config/opencode", ".copilot", ".cursor"] {
            std::fs::create_dir_all(home_root.join(directory)).expect("client dir");
        }
        let manifest = holder.path().join("manifest.json");
        let world = Self {
            _holder: holder,
            install_root,
            source_dir,
            manifest,
            home_root,
            state_root,
        };
        world.write_release("1.0.0");
        world
    }

    fn write_release(&self, version: &str) {
        let mut artifacts = Vec::new();
        for name in ArtifactName::ALL {
            let bytes = format!("{}@{version}", name.file_name());
            std::fs::write(self.source_dir.join(name.file_name()), &bytes).expect("artifact");
            artifacts.push(ArtifactEntry {
                name,
                version: version.to_owned(),
                sha256: sha256_hex(bytes.as_bytes()),
            });
        }
        let manifest = ArtifactManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            release: version.to_owned(),
            artifacts,
        };
        std::fs::write(
            &self.manifest,
            serde_json::to_string_pretty(&manifest).expect("manifest"),
        )
        .expect("write manifest");
    }

    fn run(&self, arguments: &[&str]) -> (Value, bool) {
        let output = Command::cargo_bin("kontor-bootstrap")
            .expect("binary")
            .args(arguments)
            .output()
            .expect("run");
        let document: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "stdout is not JSON: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        (document, output.status.success())
    }

    fn install(&self) -> Value {
        let (document, success) = self.run(&[
            "install",
            "--install-root",
            self.install_root.to_str().expect("utf8"),
            "--source-dir",
            self.source_dir.to_str().expect("utf8"),
            "--manifest",
            self.manifest.to_str().expect("utf8"),
            "--home-root",
            self.home_root.to_str().expect("utf8"),
            "--state-root",
            self.state_root.to_str().expect("utf8"),
        ]);
        assert!(success, "install failed: {document}");
        document
    }

    fn readback(&self) -> Value {
        let (document, success) = self.run(&[
            "readback",
            "--install-root",
            self.install_root.to_str().expect("utf8"),
            "--manifest",
            self.manifest.to_str().expect("utf8"),
            "--home-root",
            self.home_root.to_str().expect("utf8"),
            "--state-root",
            self.state_root.to_str().expect("utf8"),
        ]);
        assert!(success, "readback failed: {document}");
        document
    }

    fn client_result<'a>(&self, document: &'a Value, client: &str) -> &'a Value {
        document["clients"]
            .as_array()
            .expect("clients")
            .iter()
            .find(|report| report["client"] == client)
            .unwrap_or_else(|| panic!("no report for {client}: {document}"))
    }
}

fn assert_redacted(document: &Value, world: &World) {
    let text = document.to_string();
    for path in [
        &world.install_root,
        &world.source_dir,
        &world.home_root,
        &world.state_root,
    ] {
        let rendered = path.to_str().expect("utf8");
        assert!(
            !text.contains(rendered),
            "receipt leaked the host path {rendered}"
        );
    }
    for forbidden in ["program", "args", "KONTOR_AUTH", "credential"] {
        assert!(
            !text.contains(forbidden),
            "receipt leaked the field name {forbidden}"
        );
    }
}

#[test]
fn install_then_readback_then_replay_and_update() {
    let world = World::new();
    let document = world.install();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["release"], "1.0.0");
    let artifacts = document["artifacts"].as_array().expect("artifacts");
    assert_eq!(artifacts.len(), 4);
    assert!(
        artifacts
            .iter()
            .all(|entry| entry["disposition"] == "installed")
    );
    for client in ["codex", "opencode", "copilot", "cursor"] {
        assert_eq!(
            world.client_result(&document, client)["result"]["result"],
            "installed",
            "{client}"
        );
    }
    assert_eq!(
        world.client_result(&document, "claude-code")["result"]["result"],
        "absent",
        "the command boundary is not wired in this unit"
    );
    assert_redacted(&document, &world);
    assert!(world.state_root.join("bootstrap-receipt.json").exists());
    for name in ArtifactName::ALL {
        assert_eq!(
            std::fs::read_to_string(world.install_root.join(name.file_name())).expect("read"),
            format!("{}@1.0.0", name.file_name())
        );
    }

    let observed = world.readback();
    for client in ["codex", "opencode", "copilot", "cursor"] {
        assert_eq!(
            world.client_result(&observed, client)["state"]["state"],
            "current",
            "{client}"
        );
    }
    assert_redacted(&observed, &world);

    let replay = world.install();
    assert!(
        replay["artifacts"]
            .as_array()
            .expect("artifacts")
            .iter()
            .all(|entry| entry["disposition"] == "already_current")
    );
    for client in ["codex", "opencode", "copilot", "cursor"] {
        assert_eq!(
            world.client_result(&replay, client)["result"]["result"],
            "already_current",
            "{client}"
        );
    }

    world.write_release("2.0.0");
    let updated = world.install();
    assert!(
        updated["artifacts"]
            .as_array()
            .expect("artifacts")
            .iter()
            .all(|entry| entry["disposition"] == "updated")
    );
    assert_eq!(
        std::fs::read_to_string(world.install_root.join("kontor")).expect("read"),
        "kontor@2.0.0"
    );
}

#[test]
fn repair_through_the_cli_requires_the_exact_observed_hash() {
    let world = World::new();
    world.install();
    std::fs::write(
        world.home_root.join(".cursor/mcp.json"),
        r#"{"mcpServers": {"kontor": {"command": "/usr/bin/other-mcp"}}}"#,
    )
    .expect("seed unrelated");
    let observed = world.readback();
    let report = world.client_result(&observed, "cursor");
    assert_eq!(report["state"]["state"], "unrelated");
    let hash = report["state"]["observed"]
        .as_str()
        .expect("observed hash")
        .to_owned();

    let (wrong, success) = world.run(&[
        "repair",
        "--install-root",
        world.install_root.to_str().expect("utf8"),
        "--home-root",
        world.home_root.to_str().expect("utf8"),
        "--state-root",
        world.state_root.to_str().expect("utf8"),
        "--client",
        "cursor",
        "--expected-hash",
        &"a".repeat(64),
    ]);
    assert!(success);
    assert_eq!(wrong["result"]["result"], "conflict");
    assert_eq!(wrong["result"]["reason"], "observed_hash_mismatch");

    let (repaired, success) = world.run(&[
        "repair",
        "--install-root",
        world.install_root.to_str().expect("utf8"),
        "--home-root",
        world.home_root.to_str().expect("utf8"),
        "--state-root",
        world.state_root.to_str().expect("utf8"),
        "--client",
        "cursor",
        "--expected-hash",
        &hash,
    ]);
    assert!(success);
    assert_eq!(repaired["result"]["result"], "repaired");
    assert_redacted(&repaired, &world);
    let text = std::fs::read_to_string(world.home_root.join(".cursor/mcp.json")).expect("read");
    assert!(text.contains("kontor-mcp"));
    assert!(!text.contains("/usr/bin/other-mcp"));
}

#[test]
fn recover_is_typed_when_nothing_is_pending() {
    let world = World::new();
    world.install();
    let (document, success) = world.run(&[
        "recover",
        "--install-root",
        world.install_root.to_str().expect("utf8"),
        "--manifest",
        world.manifest.to_str().expect("utf8"),
    ]);
    assert!(success);
    assert_eq!(document["recovery"], "nothing");
    assert_redacted(&document, &world);
}

#[test]
fn an_invalid_manifest_is_a_typed_refusal_with_no_writes() {
    let world = World::new();
    std::fs::write(&world.manifest, "{}").expect("manifest");
    let (document, success) = world.run(&[
        "install",
        "--install-root",
        world.install_root.to_str().expect("utf8"),
        "--source-dir",
        world.source_dir.to_str().expect("utf8"),
        "--manifest",
        world.manifest.to_str().expect("utf8"),
        "--home-root",
        world.home_root.to_str().expect("utf8"),
        "--state-root",
        world.state_root.to_str().expect("utf8"),
    ]);
    assert!(!success);
    assert_eq!(document["error"]["code"], "manifest_invalid");
    assert!(!world.install_root.join("kontor").exists());
}

#[cfg(unix)]
#[test]
fn a_symlinked_client_config_is_a_typed_conflict() {
    use std::os::unix::fs::symlink;
    let world = World::new();
    let outside = world._holder.path().join("outside.toml");
    std::fs::write(&outside, "unrelated = true").expect("outside");
    symlink(&outside, world.home_root.join(".codex/config.toml")).expect("symlink");
    let document = world.install();
    let report = world.client_result(&document, "codex");
    assert_eq!(report["result"]["result"], "conflict");
    assert_eq!(report["result"]["reason"], "symlink");
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read"),
        "unrelated = true"
    );
}

#[test]
fn a_relative_root_is_a_typed_refusal() {
    let world = World::new();
    let (document, success) = world.run(&[
        "install",
        "--install-root",
        "relative/install",
        "--source-dir",
        world.source_dir.to_str().expect("utf8"),
        "--manifest",
        world.manifest.to_str().expect("utf8"),
        "--home-root",
        world.home_root.to_str().expect("utf8"),
        "--state-root",
        world.state_root.to_str().expect("utf8"),
    ]);
    assert!(!success);
    assert_eq!(document["error"]["code"], "install_root_invalid");
}

#[test]
fn the_binary_does_not_touch_a_real_home_by_default() {
    // Every root is required; omitting one is a usage error, never a fallback
    // to a home directory.
    let output = Command::cargo_bin("kontor-bootstrap")
        .expect("binary")
        .args(["install"])
        .output()
        .expect("run");
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("--install-root"), "{text}");
}

#[test]
fn no_process_or_service_manager_is_reachable_from_the_binary() {
    let world = World::new();
    let document = world.install();
    let text = document.to_string();
    for forbidden in ["launchctl", "systemctl", "systemd"] {
        assert!(!text.contains(forbidden));
    }
}

fn _unused(_: &Path) {}
