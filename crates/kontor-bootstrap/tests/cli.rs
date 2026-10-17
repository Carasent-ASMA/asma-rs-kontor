//! Binary-level qualification over synthetic roots: the real `kontor-bootstrap`
//! executable, assert_cmd, full prevalidation, and a redaction check on
//! everything it prints.

use std::path::PathBuf;

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

    fn run(&self, arguments: &[&str]) -> (Value, bool, String) {
        let output = Command::cargo_bin("kontor-bootstrap")
            .expect("binary")
            .args(arguments)
            .output()
            .expect("run");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let document: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!("stdout is not JSON: {stdout}");
        });
        (document, output.status.success(), stdout)
    }

    fn run_with_stderr(&self, arguments: &[&str]) -> (Value, bool, String, String) {
        let output = Command::cargo_bin("kontor-bootstrap")
            .expect("binary")
            .args(arguments)
            .output()
            .expect("run");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let document: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!("stdout is not JSON: {stdout}");
        });
        (document, output.status.success(), stdout, stderr)
    }

    fn common(&self) -> Vec<String> {
        vec![
            "--install-root".to_owned(),
            self.install_root.to_str().expect("utf8").to_owned(),
            "--manifest".to_owned(),
            self.manifest.to_str().expect("utf8").to_owned(),
            "--home-root".to_owned(),
            self.home_root.to_str().expect("utf8").to_owned(),
            "--state-root".to_owned(),
            self.state_root.to_str().expect("utf8").to_owned(),
        ]
    }

    fn install(&self) -> Value {
        let mut arguments = vec!["install".to_owned()];
        arguments.extend(self.common());
        arguments.push("--source-dir".to_owned());
        arguments.push(self.source_dir.to_str().expect("utf8").to_owned());
        let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let (document, success, stdout) = self.run(&refs);
        assert!(success, "install failed: {document} {stdout}");
        assert_redacted(&document, self);
        document
    }

    fn readback(&self) -> Value {
        let mut arguments = vec!["readback".to_owned()];
        arguments.extend(self.common());
        let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let (document, success, stdout) = self.run(&refs);
        assert!(success, "readback failed: {document} {stdout}");
        assert_redacted(&document, self);
        document
    }

    fn client_report<'a>(&self, document: &'a Value, client: &str) -> &'a Value {
        document["clients"]
            .as_array()
            .expect("clients")
            .iter()
            .find(|report| report["client"] == client)
            .unwrap_or_else(|| panic!("no report for {client}: {document}"))
    }

    fn assert_no_mutations(&self) {
        assert!(
            !self.install_root.exists(),
            "the install root was created before validation completed"
        );
    }
}

fn assert_redacted(document: &Value, world: &World) {
    let stdout = document.to_string();
    for path in [
        &world.install_root,
        &world.source_dir,
        &world.home_root,
        &world.state_root,
    ] {
        let rendered = path.to_str().expect("utf8");
        assert!(
            !stdout.contains(rendered),
            "receipt leaked the host path {rendered}"
        );
    }
    for forbidden in ["program", "\"args\"", "KONTOR_AUTH", "credential-tier"] {
        assert!(
            !stdout.contains(forbidden),
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
    assert!(
        document["artifacts"]
            .as_array()
            .expect("artifacts")
            .iter()
            .all(|entry| entry["disposition"] == "installed")
    );
    for client in ["codex", "opencode", "copilot", "cursor"] {
        assert_eq!(
            world.client_report(&document, client)["result"]["result"],
            "installed",
            "{client}"
        );
    }
    assert_eq!(
        world.client_report(&document, "claude-code")["result"]["result"],
        "unsupported"
    );
    assert!(world.state_root.join("bootstrap-receipt.json").exists());
    assert!(world.state_root.join("bootstrap-ownership.json").exists());
    let active = world.install_root.join("current");
    assert_eq!(
        std::fs::read_to_string(active.join("kontor")).expect("artifact"),
        "kontor@1.0.0"
    );

    let observed = world.readback();
    for client in ["codex", "opencode", "copilot", "cursor"] {
        assert_eq!(
            world.client_report(&observed, client)["state"]["state"],
            "current",
            "{client}"
        );
    }
    assert_eq!(
        world.client_report(&observed, "claude-code")["state"]["state"],
        "unsupported"
    );

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
            world.client_report(&replay, client)["result"]["result"],
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
        std::fs::read_to_string(world.install_root.join("current/kontor")).expect("artifact"),
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
    let report = world.client_report(&observed, "cursor");
    assert_eq!(report["state"]["state"], "unrelated");
    let hash = report["state"]["observed"]
        .as_str()
        .expect("observed hash")
        .to_owned();

    let wrong = vec![
        "repair".to_owned(),
        "--install-root".to_owned(),
        world.install_root.to_str().expect("utf8").to_owned(),
        "--home-root".to_owned(),
        world.home_root.to_str().expect("utf8").to_owned(),
        "--state-root".to_owned(),
        world.state_root.to_str().expect("utf8").to_owned(),
        "--client".to_owned(),
        "cursor".to_owned(),
        "--expected-hash".to_owned(),
        "a".repeat(64),
    ];
    let refs: Vec<&str> = wrong.iter().map(String::as_str).collect();
    let (document, success, _) = world.run(&refs);
    assert!(success);
    assert_eq!(document["result"]["result"], "conflict");
    assert_eq!(document["result"]["reason"], "observed_hash_mismatch");

    let repaired = vec![
        "repair".to_owned(),
        "--install-root".to_owned(),
        world.install_root.to_str().expect("utf8").to_owned(),
        "--home-root".to_owned(),
        world.home_root.to_str().expect("utf8").to_owned(),
        "--state-root".to_owned(),
        world.state_root.to_str().expect("utf8").to_owned(),
        "--client".to_owned(),
        "cursor".to_owned(),
        "--expected-hash".to_owned(),
        hash,
    ];
    let refs: Vec<&str> = repaired.iter().map(String::as_str).collect();
    let (document, success, _) = world.run(&refs);
    assert!(success);
    assert_eq!(document["result"]["result"], "repaired");
    let text = std::fs::read_to_string(world.home_root.join(".cursor/mcp.json")).expect("read");
    assert!(text.contains("kontor-mcp"));
    assert!(!text.contains("/usr/bin/other-mcp"));
}

#[test]
fn recover_is_typed_when_nothing_is_pending() {
    let world = World::new();
    world.install();
    let (document, success, _) = world.run(&[
        "recover",
        "--install-root",
        world.install_root.to_str().expect("utf8"),
        "--manifest",
        world.manifest.to_str().expect("utf8"),
    ]);
    assert!(success);
    assert_eq!(document["recovery"], "nothing");
}

#[test]
fn an_invalid_manifest_is_a_typed_refusal_with_no_writes() {
    let world = World::new();
    std::fs::write(&world.manifest, "{}").expect("manifest");
    let mut arguments = vec!["install".to_owned()];
    arguments.extend(world.common());
    arguments.push("--source-dir".to_owned());
    arguments.push(world.source_dir.to_str().expect("utf8").to_owned());
    let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (document, success, _) = world.run(&refs);
    assert!(!success);
    assert_eq!(document["error"]["code"], "manifest_invalid");
    world.assert_no_mutations();
}

#[test]
fn an_injected_manifest_is_refused_without_reflection() {
    let world = World::new();
    let document = serde_json::json!({
        "schema_version": 1,
        "release": "../../etc/passwd",
        "artifacts": [],
    });
    std::fs::write(
        &world.manifest,
        serde_json::to_string(&document).expect("json"),
    )
    .expect("manifest");
    let mut arguments = vec!["install".to_owned()];
    arguments.extend(world.common());
    arguments.push("--source-dir".to_owned());
    arguments.push(world.source_dir.to_str().expect("utf8").to_owned());
    let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (error, success, stdout) = world.run(&refs);
    assert!(!success);
    assert_eq!(error["error"]["code"], "manifest_invalid");
    for marker in ["passwd", "etc", "../"] {
        assert!(
            !stdout.contains(marker),
            "error reflected {marker}: {stdout}"
        );
    }
    world.assert_no_mutations();
}

#[test]
fn prevalidation_produces_zero_mutations() {
    // Relative state root.
    let world = World::new();
    let relative = vec![
        "install".to_owned(),
        "--install-root".to_owned(),
        world.install_root.to_str().expect("utf8").to_owned(),
        "--source-dir".to_owned(),
        world.source_dir.to_str().expect("utf8").to_owned(),
        "--manifest".to_owned(),
        world.manifest.to_str().expect("utf8").to_owned(),
        "--home-root".to_owned(),
        world.home_root.to_str().expect("utf8").to_owned(),
        "--state-root".to_owned(),
        "relative/state".to_owned(),
    ];
    let refs: Vec<&str> = relative.iter().map(String::as_str).collect();
    let (document, success, _) = world.run(&refs);
    assert!(!success);
    assert_eq!(document["error"]["code"], "state_root_invalid");
    world.assert_no_mutations();

    // Unknown client.
    let world = World::new();
    let mut arguments = vec!["install".to_owned()];
    arguments.extend(world.common());
    arguments.push("--source-dir".to_owned());
    arguments.push(world.source_dir.to_str().expect("utf8").to_owned());
    arguments.push("--client".to_owned());
    arguments.push("not-a-client".to_owned());
    let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (document, success, _) = world.run(&refs);
    assert!(!success);
    assert_eq!(document["error"]["code"], "unknown_client");
    world.assert_no_mutations();
}

#[cfg(unix)]
#[test]
fn a_symlinked_state_root_produces_zero_mutations() {
    use std::os::unix::fs::symlink;
    let world = World::new();
    let outside = world._holder.path().join("outside-state");
    std::fs::create_dir_all(&outside).expect("outside");
    let link = world._holder.path().join("state-link");
    symlink(&outside, &link).expect("link");
    let mut arguments = vec!["install".to_owned()];
    arguments.extend(world.common());
    arguments.push("--source-dir".to_owned());
    arguments.push(world.source_dir.to_str().expect("utf8").to_owned());
    let position = arguments
        .iter()
        .position(|argument| argument == world.state_root.to_str().expect("utf8"))
        .expect("state root");
    arguments[position] = link.to_str().expect("utf8").to_owned();
    let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (document, success, _) = world.run(&refs);
    assert!(!success);
    assert_eq!(document["error"]["code"], "state_root_invalid");
    world.assert_no_mutations();
    assert!(
        std::fs::read_dir(&outside)
            .expect("outside")
            .next()
            .is_none()
    );
}

#[test]
fn an_unknown_client_is_refused_without_reflection() {
    let world = World::new();
    let mut arguments = vec!["install".to_owned()];
    arguments.extend(world.common());
    arguments.push("--source-dir".to_owned());
    arguments.push(world.source_dir.to_str().expect("utf8").to_owned());
    arguments.push("--client".to_owned());
    arguments.push("token/secret-value".to_owned());
    let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (document, success, stdout) = world.run(&refs);
    assert!(!success);
    assert_eq!(document["error"]["code"], "unknown_client");
    for marker in ["token", "secret-value", "/"] {
        assert!(
            !stdout.contains(marker),
            "error reflected {marker}: {stdout}"
        );
    }
    world.assert_no_mutations();
}

#[test]
fn the_binary_requires_every_root_explicitly() {
    let output = Command::cargo_bin("kontor-bootstrap")
        .expect("binary")
        .args(["install"])
        .output()
        .expect("run");
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("--install-root"), "{text}");
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
    let report = world.client_report(&document, "codex");
    assert_eq!(report["result"]["result"], "conflict");
    assert_eq!(report["result"]["reason"], "symlink");
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read"),
        "unrelated = true"
    );
}

#[cfg(unix)]
#[test]
fn a_failed_client_still_writes_a_receipt_and_the_run_continues() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new();
    let codex = world.home_root.join(".codex");
    let mut permissions = std::fs::metadata(&codex).expect("metadata").permissions();
    permissions.set_mode(0o500);
    std::fs::set_permissions(&codex, permissions).expect("chmod");
    let mut arguments = vec!["install".to_owned()];
    arguments.extend(world.common());
    arguments.push("--source-dir".to_owned());
    arguments.push(world.source_dir.to_str().expect("utf8").to_owned());
    let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (document, success, _) = world.run(&refs);
    let mut restore = std::fs::metadata(&codex).expect("metadata").permissions();
    restore.set_mode(0o700);
    std::fs::set_permissions(&codex, restore).expect("restore");

    assert!(!success, "a client failure exits non-zero");
    assert_eq!(document["error"]["code"], "client_adapter_failed");
    let receipt_path = world.state_root.join("bootstrap-receipt.json");
    assert!(receipt_path.exists(), "the receipt is always written");
    let receipt: Value =
        serde_json::from_str(&std::fs::read_to_string(&receipt_path).expect("receipt"))
            .expect("receipt json");
    let reports = receipt["clients"].as_array().expect("clients");
    let codex = reports
        .iter()
        .find(|report| report["client"] == "codex")
        .expect("codex report");
    assert_eq!(codex["mode"], "failed");
    assert_eq!(codex["reason"], "confinement");
    for client in ["opencode", "copilot", "cursor"] {
        let report = reports
            .iter()
            .find(|report| report["client"] == client)
            .unwrap_or_else(|| panic!("no report for {client}: {receipt}"));
        assert_eq!(report["result"]["result"], "installed", "{client}");
    }
    assert!(world.state_root.join("bootstrap-ownership.json").exists());
    assert!(
        std::fs::read_to_string(world.home_root.join(".config/opencode/opencode.json"))
            .expect("opencode config")
            .contains("kontor-mcp")
    );
}

#[test]
fn refusals_do_not_reflect_valid_character_markers_in_either_stream() {
    let marker = "SYNTHETICMARKER_TOKEN";

    // install: a syntactically valid but incoherent artifact version token.
    let world = World::new();
    let document = serde_json::json!({
        "schema_version": MANIFEST_SCHEMA_VERSION,
        "release": "1.0.0",
        "artifacts": ArtifactName::ALL
            .into_iter()
            .map(|name| serde_json::json!({
                "name": name,
                "version": marker,
                "sha256": "a".repeat(64),
            }))
            .collect::<Vec<_>>(),
    });
    std::fs::write(
        &world.manifest,
        serde_json::to_string(&document).expect("json"),
    )
    .expect("manifest");
    let mut arguments = vec!["install".to_owned()];
    arguments.extend(world.common());
    arguments.push("--source-dir".to_owned());
    arguments.push(world.source_dir.to_str().expect("utf8").to_owned());
    let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (document, success, stdout, stderr) = world.run_with_stderr(&refs);
    assert!(!success);
    assert_eq!(document["error"]["code"], "manifest_invalid");
    assert!(
        !stdout.contains(marker),
        "install stdout reflected the marker: {stdout}"
    );
    assert!(
        !stderr.contains(marker),
        "install stderr reflected the marker: {stderr}"
    );
    world.assert_no_mutations();

    // readback: a valid-character marker in an injected root path must not
    // appear anywhere in the emitted receipt.
    let world = World::new();
    let marker_state = world._holder.path().join("SYNTHETICMARKER_STATE");
    std::fs::create_dir_all(&marker_state).expect("marker state");
    let readback = vec![
        "readback".to_owned(),
        "--install-root".to_owned(),
        world.install_root.to_str().expect("utf8").to_owned(),
        "--manifest".to_owned(),
        world.manifest.to_str().expect("utf8").to_owned(),
        "--home-root".to_owned(),
        world.home_root.to_str().expect("utf8").to_owned(),
        "--state-root".to_owned(),
        marker_state.to_str().expect("utf8").to_owned(),
    ];
    let refs: Vec<&str> = readback.iter().map(String::as_str).collect();
    let (_, success, stdout, stderr) = world.run_with_stderr(&refs);
    assert!(success, "readback over a marker root succeeds: {stdout}");
    assert!(
        !stdout.contains("SYNTHETICMARKER_STATE"),
        "readback stdout reflected the marker: {stdout}"
    );
    assert!(
        !stderr.contains("SYNTHETICMARKER_STATE"),
        "readback stderr reflected the marker: {stderr}"
    );

    // repair: a valid-character but non-hex expected hash is refused without
    // echoing the supplied token.
    let world = World::new();
    let repair = vec![
        "repair".to_owned(),
        "--home-root".to_owned(),
        world.home_root.to_str().expect("utf8").to_owned(),
        "--state-root".to_owned(),
        world.state_root.to_str().expect("utf8").to_owned(),
        "--install-root".to_owned(),
        world.install_root.to_str().expect("utf8").to_owned(),
        "--client".to_owned(),
        "cursor".to_owned(),
        "--expected-hash".to_owned(),
        marker.to_owned(),
    ];
    let refs: Vec<&str> = repair.iter().map(String::as_str).collect();
    let (document, success, stdout, stderr) = world.run_with_stderr(&refs);
    assert!(!success);
    assert_eq!(document["error"]["code"], "invalid_hash");
    assert!(
        !stdout.contains(marker),
        "repair stdout reflected the marker: {stdout}"
    );
    assert!(
        !stderr.contains(marker),
        "repair stderr reflected the marker: {stderr}"
    );

    // A valid-character unknown client is refused without reflection too.
    let world = World::new();
    let unknown = vec![
        "readback".to_owned(),
        "--install-root".to_owned(),
        world.install_root.to_str().expect("utf8").to_owned(),
        "--manifest".to_owned(),
        world.manifest.to_str().expect("utf8").to_owned(),
        "--home-root".to_owned(),
        world.home_root.to_str().expect("utf8").to_owned(),
        "--state-root".to_owned(),
        world.state_root.to_str().expect("utf8").to_owned(),
        "--client".to_owned(),
        marker.to_owned(),
    ];
    let refs: Vec<&str> = unknown.iter().map(String::as_str).collect();
    let (document, success, stdout, stderr) = world.run_with_stderr(&refs);
    assert!(!success);
    assert_eq!(document["error"]["code"], "unknown_client");
    assert!(
        !stdout.contains(marker),
        "unknown-client stdout reflected the marker: {stdout}"
    );
    assert!(
        !stderr.contains(marker),
        "unknown-client stderr reflected the marker: {stderr}"
    );
}

#[test]
fn error_stream_branches_do_not_reflect_markers() {
    let marker = "SYNTHETICMARKER_BRANCH";

    // repair with an unknown client: the client token is never echoed.
    let world = World::new();
    let repair = vec![
        "repair".to_owned(),
        "--home-root".to_owned(),
        world.home_root.to_str().expect("utf8").to_owned(),
        "--state-root".to_owned(),
        world.state_root.to_str().expect("utf8").to_owned(),
        "--install-root".to_owned(),
        world.install_root.to_str().expect("utf8").to_owned(),
        "--client".to_owned(),
        marker.to_owned(),
        "--expected-hash".to_owned(),
        "a".repeat(64),
    ];
    let refs: Vec<&str> = repair.iter().map(String::as_str).collect();
    let (document, success, stdout, stderr) = world.run_with_stderr(&refs);
    assert!(!success);
    assert_eq!(document["error"]["code"], "unknown_client");
    assert!(
        !stdout.contains(marker),
        "repair stdout reflected {marker}: {stdout}"
    );
    assert!(
        !stderr.contains(marker),
        "repair stderr reflected {marker}: {stderr}"
    );

    // An injected-root validation refusal is its own branch: the
    // marker-named symlinked root is refused before any receipt or output
    // work, with constant text in both streams. It is a root-validation
    // witness, not a persisted-output I/O witness; genuine receipt/output
    // I/O failures remain a separate UNWITNESSED branch.
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let world = World::new();
        let outside = world._holder.path().join("outside root");
        std::fs::create_dir_all(&outside).expect("outside");
        let link = world._holder.path().join(marker);
        symlink(&outside, &link).expect("symlink root");
        let repair = vec![
            "repair".to_owned(),
            "--home-root".to_owned(),
            link.to_str().expect("utf8").to_owned(),
            "--state-root".to_owned(),
            world.state_root.to_str().expect("utf8").to_owned(),
            "--install-root".to_owned(),
            world.install_root.to_str().expect("utf8").to_owned(),
            "--client".to_owned(),
            "cursor".to_owned(),
            "--expected-hash".to_owned(),
            "a".repeat(64),
        ];
        let refs: Vec<&str> = repair.iter().map(String::as_str).collect();
        let (document, success, stdout, stderr) = world.run_with_stderr(&refs);
        assert!(!success);
        assert_eq!(document["error"]["code"], "home_root_invalid");
        assert!(
            !stdout.contains(marker),
            "root-refusal stdout reflected {marker}: {stdout}"
        );
        assert!(
            !stderr.contains(marker),
            "root-refusal stderr reflected {marker}: {stderr}"
        );
    }
}
