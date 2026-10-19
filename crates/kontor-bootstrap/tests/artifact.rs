//! Artifact-layer security regressions: strict journal validation, generation
//! activation without mixed active sets, confined promotion, and recovery that
//! removes or overwrites a name only when an observation at that boundary
//! proves it is bootstrap's own; any mismatch or observation error preserves
//! the name. The accepted same-UID observation-to-effect residual remains:
//! bytes can change between an observation and the by-name effect.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kontor_bootstrap::artifact::manifest::{
    ArtifactEntry, ArtifactManifest, ArtifactName, MANIFEST_SCHEMA_VERSION, ManifestError,
    VerifiedSource, sha256_hex,
};
use kontor_bootstrap::confinement::{Dir, MAX_DOCUMENT_BYTES};
use kontor_bootstrap::fault::{FailPoint, FaultInjector, NoFaults, ScriptedFaults};
use kontor_bootstrap::{InstallError, InstallOutcome, Installer, RecoveryOutcome};

const INTERNAL: &str = ".kontor-bootstrap";

struct Release {
    _holder: tempfile::TempDir,
    directory: PathBuf,
    manifest: ArtifactManifest,
    sources: Vec<VerifiedSource>,
}

fn release(version: &str) -> Release {
    let holder = tempfile::tempdir().expect("source holder");
    let directory = holder.path().join(format!("release {version}"));
    std::fs::create_dir_all(&directory).expect("source dir");
    let mut artifacts = Vec::new();
    for name in ArtifactName::ALL {
        let bytes = format!("{}@{version}", name.file_name());
        std::fs::write(directory.join(name.file_name()), &bytes).expect("artifact");
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
    let sources = manifest
        .verify_sources(&directory)
        .expect("verified sources");
    Release {
        _holder: holder,
        directory,
        manifest,
        sources,
    }
}

impl Release {
    fn dir(&self) -> Dir {
        Dir::open_root(&self.directory).expect("source dir")
    }
}

fn install(installer: &Installer, release: &Release) -> Result<InstallOutcome, InstallError> {
    installer.install(
        &release.manifest,
        &release.sources,
        &release.dir(),
        &mut NoFaults,
    )
}

fn installer(root: &Path) -> Installer {
    Installer::at(root).expect("installer")
}

/// Every artifact as it reads through the active pointer, or `None` when no
/// pointer is active.
fn active_set(root: &Path) -> Option<BTreeMap<String, String>> {
    let pointer = std::fs::read_link(root.join("current")).ok()?;
    let generation = root.join(pointer);
    let mut contents = BTreeMap::new();
    for name in ArtifactName::ALL {
        let text = std::fs::read_to_string(generation.join(name.file_name())).ok()?;
        contents.insert(name.file_name().to_owned(), text);
    }
    Some(contents)
}

fn assert_whole_release(root: &Path, version: &str) {
    let contents = active_set(root).expect("an active generation");
    for name in ArtifactName::ALL {
        assert_eq!(
            contents[name.file_name()],
            format!("{}@{version}", name.file_name()),
            "mixed active set"
        );
    }
}

fn assert_old_or_new(root: &Path, old: &str, new: &str) {
    let Some(contents) = active_set(root) else {
        return;
    };
    let old: Vec<String> = ArtifactName::ALL
        .into_iter()
        .map(|name| format!("{}@{old}", name.file_name()))
        .collect();
    let new: Vec<String> = ArtifactName::ALL
        .into_iter()
        .map(|name| format!("{}@{new}", name.file_name()))
        .collect();
    let observed: Vec<String> = ArtifactName::ALL
        .into_iter()
        .map(|name| contents[name.file_name()].clone())
        .collect();
    assert!(
        observed == old || observed == new,
        "active set is neither whole-old nor whole-new: {observed:?}"
    );
}

fn internal_entries(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join(INTERNAL))
        .map(|entries| {
            entries
                .map(|entry| {
                    entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn recursive_names(root: &Path) -> Vec<String> {
    fn walk(root: &Path, prefix: &Path, names: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = prefix.join(&name);
            names.push(relative.to_string_lossy().into_owned());
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                walk(&entry.path(), &relative, names);
            }
        }
    }
    let mut names = Vec::new();
    walk(root, Path::new(""), &mut names);
    names.sort();
    names
}

type HookAction = Box<dyn FnMut(&Path) -> Result<(), InstallError>>;

struct Hooks(Vec<(FailPoint, HookAction)>);

impl FaultInjector for Hooks {
    fn hit(&mut self, point: FailPoint, root: &Path) -> Result<(), InstallError> {
        for (target, action) in &mut self.0 {
            if *target == point {
                action(root)?;
            }
        }
        Ok(())
    }
}

fn hook(
    point: FailPoint,
    action: impl FnMut(&Path) -> Result<(), InstallError> + 'static,
) -> (FailPoint, HookAction) {
    (point, Box::new(action))
}

#[test]
fn a_clean_install_activates_one_generation_and_replay_writes_nothing() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let source_before = std::fs::read_dir(&version_one.directory)
        .expect("source")
        .count();
    let installer = installer(root_holder.path());
    assert_eq!(
        install(&installer, &version_one),
        Ok(InstallOutcome::Installed)
    );
    assert_whole_release(root_holder.path(), "1.0.0");
    assert_eq!(
        internal_entries(root_holder.path()),
        vec!["lock".to_owned()]
    );
    assert_eq!(
        install(&installer, &version_one),
        Ok(InstallOutcome::AlreadyCurrent)
    );
    assert_eq!(
        std::fs::read_dir(&version_one.directory)
            .expect("source")
            .count(),
        source_before
    );
}

#[cfg(unix)]
#[test]
fn a_library_install_on_a_group_or_world_writable_root_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let root_holder = tempfile::tempdir().expect("root");
    let root = root_holder.path().join("install");
    std::fs::create_dir(&root).expect("install root");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).expect("chmod");
    let version_one = release("1.0.0");
    let installer = installer(&root);

    assert_eq!(
        install(&installer, &version_one),
        Err(InstallError::InsecureRoot),
        "a caller that skips the CLI still gets the private-root refusal"
    );
    assert_eq!(
        std::fs::read_dir(&root).expect("root").count(),
        0,
        "the refusal happens before any effect"
    );
}

#[cfg(unix)]
#[test]
fn a_writable_active_generation_is_never_trusted() {
    use std::os::unix::fs::PermissionsExt;

    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let installer = installer(root_holder.path());
    assert_eq!(
        install(&installer, &version_one),
        Ok(InstallOutcome::Installed)
    );
    let pointer = std::fs::read_link(root_holder.path().join("current")).expect("pointer");
    let generation = root_holder.path().join(pointer);
    std::fs::set_permissions(&generation, std::fs::Permissions::from_mode(0o777))
        .expect("chmod generation");

    assert_eq!(
        install(&installer, &version_one),
        Err(InstallError::InsecureRoot),
        "another local user could replace the verified binaries"
    );
    assert_eq!(
        installer.status(&version_one.manifest),
        Err(InstallError::InsecureRoot)
    );
    assert_eq!(
        installer.active_generation(),
        Err(InstallError::InsecureRoot)
    );
}

#[cfg(unix)]
#[test]
fn a_writable_managed_artifact_is_never_trusted() {
    use std::os::unix::fs::PermissionsExt;

    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let installer = installer(root_holder.path());
    assert_eq!(
        install(&installer, &version_one),
        Ok(InstallOutcome::Installed)
    );
    let pointer = std::fs::read_link(root_holder.path().join("current")).expect("pointer");
    let mcp = root_holder
        .path()
        .join(pointer)
        .join(ArtifactName::Mcp.file_name());
    std::fs::set_permissions(&mcp, std::fs::Permissions::from_mode(0o777)).expect("chmod mcp");

    assert_eq!(
        install(&installer, &version_one),
        Err(InstallError::InsecureRoot)
    );
}

#[cfg(unix)]
#[test]
fn verify_active_for_execution_refuses_a_shared_root_or_releases() {
    use std::os::unix::fs::PermissionsExt;

    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let installer = installer(root_holder.path());
    assert_eq!(
        install(&installer, &version_one),
        Ok(InstallOutcome::Installed)
    );
    assert_eq!(installer.verify_active_for_execution(), Ok(()));

    let releases = root_holder.path().join("releases");
    std::fs::set_permissions(&releases, std::fs::Permissions::from_mode(0o777))
        .expect("chmod releases");
    assert_eq!(
        installer.verify_active_for_execution(),
        Err(InstallError::InsecureRoot)
    );
    std::fs::set_permissions(&releases, std::fs::Permissions::from_mode(0o755))
        .expect("chmod releases");

    std::fs::set_permissions(root_holder.path(), std::fs::Permissions::from_mode(0o777))
        .expect("chmod root");
    assert_eq!(
        installer.verify_active_for_execution(),
        Err(InstallError::InsecureRoot)
    );
}

#[test]
#[cfg(unix)]
fn a_lost_owner_execute_bit_is_repaired_before_already_current() {
    use std::os::unix::fs::PermissionsExt;

    // 0645 and 0655 keep an execute bit for another class, so a check that
    // accepts any execute bit reports a binary the owner cannot run as current.
    for lost in [0o644, 0o645, 0o655] {
        let root_holder = tempfile::tempdir().expect("root");
        let version_one = release("1.0.0");
        let installer = installer(root_holder.path());
        assert_eq!(
            install(&installer, &version_one),
            Ok(InstallOutcome::Installed)
        );
        let pointer = std::fs::read_link(root_holder.path().join("current")).expect("pointer");
        let mcp = root_holder
            .path()
            .join(pointer)
            .join(ArtifactName::Mcp.file_name());
        std::fs::set_permissions(&mcp, std::fs::Permissions::from_mode(lost)).expect("chmod");
        assert_eq!(
            install(&installer, &version_one),
            Ok(InstallOutcome::ModeRepaired(vec![ArtifactName::Mcp])),
            "mode {lost:o}"
        );
        let mode = std::fs::metadata(&mcp)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o100,
            0o100,
            "owner execute permission restored from {lost:o}"
        );
        assert_eq!(mode & 0o777, 0o755, "the repaired mode from {lost:o}");
    }
}

#[test]
#[cfg(unix)]
fn mode_repair_does_not_require_write_access_to_other_binaries() {
    use std::os::unix::fs::PermissionsExt;

    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let installer = installer(root_holder.path());
    assert_eq!(
        install(&installer, &version_one),
        Ok(InstallOutcome::Installed)
    );
    let pointer = std::fs::read_link(root_holder.path().join("current")).expect("pointer");
    let generation = root_holder.path().join(pointer);
    let cli = generation.join(ArtifactName::Cli.file_name());
    let mcp = generation.join(ArtifactName::Mcp.file_name());
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o555)).expect("chmod cli");
    std::fs::set_permissions(&mcp, std::fs::Permissions::from_mode(0o644)).expect("chmod mcp");
    assert_eq!(
        install(&installer, &version_one),
        Ok(InstallOutcome::ModeRepaired(vec![ArtifactName::Mcp]))
    );
    let mcp_mode = std::fs::metadata(&mcp)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(
        mcp_mode & 0o777,
        0o755,
        "the artifact missing owner execute is repaired"
    );
    let cli_mode = std::fs::metadata(&cli)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(
        cli_mode & 0o777,
        0o555,
        "an executable read-only sibling keeps its mode"
    );
}

#[test]
fn an_update_replaces_the_whole_active_generation() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    assert_eq!(
        install(&installer, &version_one),
        Ok(InstallOutcome::Installed)
    );
    assert_eq!(
        install(&installer, &version_two),
        Ok(InstallOutcome::Updated)
    );
    assert_whole_release(root_holder.path(), "2.0.0");
    // Rollback material is preserved, not pruned.
    let generations = std::fs::read_dir(root_holder.path().join("releases"))
        .expect("releases")
        .count();
    assert_eq!(generations, 2);
}

fn fault_points() -> Vec<FailPoint> {
    vec![
        FailPoint::BeforeLock,
        FailPoint::AfterLock,
        FailPoint::BeforeStage,
        FailPoint::AfterStage(ArtifactName::Cli),
        FailPoint::AfterStage(ArtifactName::Daemon),
        FailPoint::AfterStage(ArtifactName::Mcp),
        FailPoint::AfterStage(ArtifactName::Bootstrap),
        FailPoint::BeforeSwap(ArtifactName::Mcp),
        FailPoint::AfterSwap(ArtifactName::Mcp),
        FailPoint::AfterPromote,
        FailPoint::BeforeActivate,
        FailPoint::AfterActivationTemp,
        FailPoint::BeforeFinalize,
        FailPoint::AfterFinalize,
    ]
}

#[test]
fn a_fault_at_every_boundary_leaves_no_mixed_generation_and_recovers() {
    for point in fault_points() {
        let root_holder = tempfile::tempdir().expect("root");
        let version_one = release("1.0.0");
        let version_two = release("2.0.0");
        let installer = installer(root_holder.path());
        install(&installer, &version_one).expect("baseline");
        let error = installer
            .install(
                &version_two.manifest,
                &version_two.sources,
                &version_two.dir(),
                &mut ScriptedFaults::failing_at(point.clone()),
            )
            .expect_err("scripted fault");
        assert_eq!(
            error,
            InstallError::Injected {
                point: point.clone()
            }
        );
        assert_old_or_new(root_holder.path(), "1.0.0", "2.0.0");

        let outcome = installer.recover(&mut NoFaults).expect("recovery");
        let expect_new = matches!(
            point,
            FailPoint::BeforeSwap(_)
                | FailPoint::AfterSwap(_)
                | FailPoint::AfterPromote
                | FailPoint::BeforeActivate
                | FailPoint::AfterActivationTemp
                | FailPoint::BeforeFinalize
                | FailPoint::AfterFinalize
        );
        let expected_outcome = if matches!(
            point,
            FailPoint::BeforeSwap(_)
                | FailPoint::AfterSwap(_)
                | FailPoint::AfterPromote
                | FailPoint::BeforeActivate
                | FailPoint::AfterActivationTemp
                | FailPoint::BeforeFinalize
        ) {
            RecoveryOutcome::Completed
        } else {
            RecoveryOutcome::Nothing
        };
        assert_eq!(outcome, expected_outcome, "recovery at {point:?}");
        if expect_new {
            assert_whole_release(root_holder.path(), "2.0.0");
        } else {
            assert_whole_release(root_holder.path(), "1.0.0");
        }
        assert_eq!(
            internal_entries(root_holder.path()),
            vec!["lock".to_owned()],
            "cleanup after {point:?}"
        );
        assert_eq!(
            installer.recover(&mut NoFaults),
            Ok(RecoveryOutcome::Nothing),
            "replay after {point:?}"
        );
    }
}

#[test]
fn a_malicious_journal_is_refused_without_effects() {
    let manifest = release("1.0.0").manifest;
    let manifest_value = serde_json::to_value(&manifest).expect("manifest json");
    let valid = serde_json::json!({
        "schema_version": 2,
        "nonce": "123-456",
        "generation": "1.0.0-123-456",
        "state": "prepared",
        "manifest": manifest_value,
        "previous": null,
    });

    let cases: Vec<(&str, serde_json::Value)> = vec![
        ("wrong schema", {
            let mut value = valid.clone();
            value["schema_version"] = serde_json::json!(99);
            value
        }),
        ("traversal nonce", {
            let mut value = valid.clone();
            value["nonce"] = serde_json::json!("../escape");
            value
        }),
        ("traversal generation", {
            let mut value = valid.clone();
            value["generation"] = serde_json::json!("../escape-123-456");
            value
        }),
        ("generation does not match", {
            let mut value = valid.clone();
            value["generation"] = serde_json::json!("2.0.0-123-456");
            value
        }),
        ("manifest with a missing artifact", {
            let mut value = valid.clone();
            value["manifest"]["artifacts"]
                .as_array_mut()
                .expect("artifacts")
                .pop();
            value
        }),
        ("manifest with traversal release", {
            let mut value = valid.clone();
            value["manifest"]["release"] = serde_json::json!("../../etc");
            value
        }),
        ("manifest with a bad digest", {
            let mut value = valid.clone();
            value["manifest"]["artifacts"][0]["sha256"] = serde_json::json!("short");
            value
        }),
        ("unformatted nonce", {
            let mut value = valid.clone();
            value["nonce"] = serde_json::json!("abc def");
            value["generation"] = serde_json::json!("1.0.0-abc def");
            value
        }),
        ("overlong nonce", {
            let mut value = valid.clone();
            value["nonce"] = serde_json::json!("123456789012345678901-1");
            value["generation"] = serde_json::json!("1.0.0-123456789012345678901-1");
            value
        }),
        ("traversal previous generation", {
            let mut value = valid.clone();
            value["previous"] = serde_json::json!({
                "generation": "../evil",
                "release": "1.0.0",
            });
            value
        }),
        ("unknown field", {
            let mut value = valid.clone();
            value["extra"] = serde_json::json!(true);
            value
        }),
        ("partial document", serde_json::json!({"schema_version": 2})),
    ];

    for (name, document) in cases {
        let root_holder = tempfile::tempdir().expect("root");
        let installer = installer(root_holder.path());
        let internal = root_holder.path().join(INTERNAL);
        std::fs::create_dir_all(&internal).expect("internal");
        let journal_path = internal.join("journal.json");
        let bytes = serde_json::to_vec(&document).expect("document");
        std::fs::write(&journal_path, &bytes).expect("journal");
        let before = recursive_names(root_holder.path());

        let error = installer
            .recover(&mut NoFaults)
            .expect_err(&format!("{name} must be refused"));
        assert!(
            matches!(
                error,
                InstallError::JournalUnsupported | InstallError::JournalInvalid
            ),
            "{name}: {error:?}"
        );
        assert_eq!(
            std::fs::read(&journal_path).expect("journal read"),
            bytes,
            "{name}: the journal was modified"
        );
        assert_eq!(
            recursive_names(root_holder.path()),
            before,
            "{name}: a filesystem effect occurred"
        );
    }
}

fn manifest_digest(manifest: &ArtifactManifest) -> String {
    use kontor_bootstrap::artifact::manifest::sha256_hex;
    sha256_hex(&serde_json::to_vec(manifest).expect("manifest bytes"))
}

fn write_journal(root: &Path, state: &str, manifest: &ArtifactManifest, nonce: &str) {
    let internal = root.join(INTERNAL);
    std::fs::create_dir_all(&internal).expect("internal");
    let generation = format!("{}-{nonce}", manifest.release);
    std::fs::write(
        internal.join("journal.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "nonce": nonce,
            "generation": generation,
            "state": state,
            "manifest": manifest,
            "previous": null,
        }))
        .expect("journal bytes"),
    )
    .expect("journal");
}

fn write_staging_owner(staging: &Path, nonce: &str, manifest: &ArtifactManifest) {
    let generation = format!("{}-{nonce}", manifest.release);
    std::fs::write(
        staging.join(".kontor-staging-owner"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "nonce": nonce,
            "generation": generation,
            "manifest_sha256": manifest_digest(manifest),
        }))
        .expect("owner bytes"),
    )
    .expect("staging owner");
}

/// Write a valid journal whose expected previous pointer is explicit.
fn write_journal_with_previous(
    root: &Path,
    state: &str,
    manifest: &ArtifactManifest,
    nonce: &str,
    previous: serde_json::Value,
) {
    let internal = root.join(INTERNAL);
    std::fs::create_dir_all(&internal).expect("internal");
    let generation = format!("{}-{nonce}", manifest.release);
    std::fs::write(
        internal.join("journal.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "nonce": nonce,
            "generation": generation,
            "state": state,
            "manifest": manifest,
            "previous": previous,
        }))
        .expect("journal bytes"),
    )
    .expect("journal");
}

/// A complete, owned staging directory: owner receipt, every artifact and a
/// coherent generation marker, exactly as a successful staging phase leaves it.
fn write_complete_staging(root: &Path, nonce: &str, manifest: &ArtifactManifest) {
    let staging = root.join(INTERNAL).join(format!("staging-{nonce}"));
    std::fs::create_dir_all(&staging).expect("staging");
    write_staging_owner(&staging, nonce, manifest);
    for name in ArtifactName::ALL {
        std::fs::write(
            staging.join(name.file_name()),
            format!("{}@{}", name.file_name(), manifest.release),
        )
        .expect("artifact");
    }
    let generation = format!("{}-{nonce}", manifest.release);
    std::fs::write(
        staging.join(".kontor-generation.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "generation": generation,
            "release": manifest.release,
            "nonce": nonce,
            "artifacts": manifest.artifacts,
        }))
        .expect("marker bytes"),
    )
    .expect("marker");
}

#[test]
fn a_prepared_journal_with_foreign_staging_bytes_is_preserved() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    let staging = root_holder.path().join(INTERNAL).join("staging-123-456");
    std::fs::create_dir_all(&staging).expect("staging");
    std::fs::write(staging.join("kontor-mcp"), b"foreign bytes").expect("foreign");
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        std::fs::read_to_string(staging.join("kontor-mcp")).expect("read"),
        "foreign bytes"
    );
    assert!(
        root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

#[test]
fn a_staging_artifact_with_tampered_bytes_is_preserved_not_removed() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    let staging = root_holder.path().join(INTERNAL).join("staging-123-456");
    std::fs::create_dir_all(&staging).expect("staging");
    write_staging_owner(&staging, "123-456", &version.manifest);
    for name in ArtifactName::ALL {
        let bytes = if name == ArtifactName::Mcp {
            b"tampered".to_vec()
        } else {
            format!("{}@1.0.0", name.file_name()).into_bytes()
        };
        std::fs::write(staging.join(name.file_name()), bytes).expect("artifact");
    }
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        std::fs::read_to_string(staging.join("kontor-mcp")).expect("read"),
        "tampered"
    );
    assert!(
        root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

#[test]
fn an_existing_staging_name_is_never_adopted() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(FailPoint::BeforeStage, move |_| {
        let internal = hook_root.join(INTERNAL);
        let journal =
            std::fs::read_to_string(internal.join("journal.json")).map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_str(&journal).map_err(|_| InstallError::Io)?;
        let nonce = value["nonce"].as_str().ok_or(InstallError::Io)?;
        let staging = internal.join(format!("staging-{nonce}"));
        std::fs::create_dir_all(&staging).map_err(|_| InstallError::Io)?;
        std::fs::write(staging.join("kontor-mcp"), b"foreign staging").map_err(|_| InstallError::Io)
    })]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("an existing staging name is never adopted");
    assert_eq!(error, InstallError::StagingTaken);
    let staged = std::fs::read_dir(root.join(INTERNAL))
        .expect("internal")
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("staging-"))
        .expect("the foreign staging is preserved");
    assert_eq!(
        std::fs::read_to_string(staged.path().join("kontor-mcp")).expect("read"),
        "foreign staging"
    );
    assert_whole_release(&root, "1.0.0");
}

#[test]
fn a_receipt_less_staging_with_matching_bytes_is_preserved() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    let staging = root_holder.path().join(INTERNAL).join("staging-123-456");
    std::fs::create_dir_all(&staging).expect("staging");
    for name in ArtifactName::ALL {
        std::fs::write(
            staging.join(name.file_name()),
            format!("{}@1.0.0", name.file_name()),
        )
        .expect("artifact");
    }
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert!(staging.join("kontor-mcp").exists());
    assert!(
        root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

#[test]
fn a_marker_that_is_not_a_regular_file_is_never_removed() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    let staging = root_holder.path().join(INTERNAL).join("staging-123-456");
    std::fs::create_dir_all(&staging).expect("staging");
    write_staging_owner(&staging, "123-456", &version.manifest);
    for name in ArtifactName::ALL {
        std::fs::write(
            staging.join(name.file_name()),
            format!("{}@1.0.0", name.file_name()),
        )
        .expect("artifact");
    }
    // A directory where the marker must be: reading it is an observation
    // error and must preserve the whole staging directory.
    std::fs::create_dir(staging.join(".kontor-generation.json")).expect("marker dir");
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert!(staging.join("kontor-mcp").exists());
    assert!(staging.join(".kontor-generation.json").is_dir());
    assert!(
        root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

/// One directory's direct-level captured inventory: name, kind, whole-file
/// content hash and per-entry identity. Test-only capture operand.
type DirectoryInventory = Vec<(String, String, String, String)>;

/// The retained staging capture: basename, original directory identity,
/// substituted directory identity, each directory's own inventory, and the
/// exact journal bytes, in capture order.
type StagingSnapshot = (
    String,
    String,
    String,
    DirectoryInventory,
    DirectoryInventory,
    Vec<u8>,
);

/// The shared capture cell used by the staging-substitution test hooks.
type StagingCapture = std::rc::Rc<std::cell::RefCell<Option<StagingSnapshot>>>;

#[test]
fn a_substituted_staging_directory_is_never_promoted() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let releases_before = directory_inventory(&root.join("releases"));
    let pointer_before = pointer_observation(&root);
    let staging_capture: StagingCapture = std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_capture = std::rc::Rc::clone(&staging_capture);
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(
        FailPoint::BeforeSwap(ArtifactName::Mcp),
        move |_| {
            let internal = hook_root.join(INTERNAL);
            let staging = std::fs::read_dir(&internal)
                .map_err(|_| InstallError::Io)?
                .filter_map(Result::ok)
                .find(|entry| entry.file_name().to_string_lossy().starts_with("staging-"))
                .ok_or(InstallError::Io)?;
            // Capture the exact pre-substitution staging identity/inventory and
            // the substituted directory's own identity/inventory immediately
            // after the copy, plus the journal bytes. Each directory is later
            // compared against its own retained snapshot; the inventories are
            // direct-level (name, kind, whole-file content hash, per-entry
            // identity) and do not recurse into nested directories.
            let identity = directory_identity(&staging.path());
            let inventory = directory_inventory(&staging.path());
            let journal =
                std::fs::read(internal.join("journal.json")).map_err(|_| InstallError::Io)?;
            let moved = internal.join("staging-moved-by-racer");
            std::fs::rename(staging.path(), &moved).map_err(|_| InstallError::Io)?;
            std::fs::create_dir(staging.path()).map_err(|_| InstallError::Io)?;
            for entry in std::fs::read_dir(&moved).map_err(|_| InstallError::Io)? {
                let entry = entry.map_err(|_| InstallError::Io)?;
                std::fs::copy(entry.path(), staging.path().join(entry.file_name()))
                    .map_err(|_| InstallError::Io)?;
            }
            let substituted_identity = directory_identity(&staging.path());
            let substituted_inventory = directory_inventory(&staging.path());
            *hook_capture.borrow_mut() = Some((
                staging.file_name().to_string_lossy().into_owned(),
                identity,
                substituted_identity,
                inventory,
                substituted_inventory,
                journal,
            ));
            Ok(())
        },
    )]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a substituted staging directory is never promoted");
    assert_eq!(error, InstallError::ActiveUnverified);
    let captured = staging_capture.borrow();
    let (
        staging_name,
        staging_identity,
        substituted_identity,
        staging_inventory,
        substituted_inventory,
        journal_snapshot,
    ) = captured.as_ref().expect("captured staging snapshot");
    // Exact generation-name absence through symlink_metadata NotFound, and an
    // unchanged, sorted release enumeration.
    let journal: serde_json::Value =
        serde_json::from_slice(journal_snapshot).expect("captured journal json");
    let generation = journal["generation"].as_str().expect("generation");
    let error = std::fs::symlink_metadata(root.join("releases").join(generation))
        .expect_err("the promoted generation name must be absent");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(
        directory_inventory(&root.join("releases")),
        releases_before,
        "the sorted release inventory is unchanged"
    );
    assert_eq!(
        pointer_observation(&root),
        pointer_before,
        "the pointer kind/identity/target is unchanged"
    );
    assert_eq!(
        &journal_bytes(&root),
        journal_snapshot,
        "the exact journal bytes are retained"
    );
    // Each staging directory is compared against its own intervention
    // snapshot: the verified original under the racer name keeps its exact
    // identity and complete byte manifest, and the substituted copy keeps its
    // own copied-entry identities and the staged bytes. Comparing the copy
    // against the original's entry identities would be invalid, because
    // std::fs::copy creates new inodes while the originals remain live.
    let moved = root.join(INTERNAL).join("staging-moved-by-racer");
    #[cfg(unix)]
    assert_eq!(
        &directory_identity(&moved),
        staging_identity,
        "the verified directory keeps its exact identity under the racer name"
    );
    let moved_now = directory_inventory(&moved);
    assert_eq!(
        &moved_now, staging_inventory,
        "the verified directory keeps its complete byte manifest"
    );
    let substituted = root.join(INTERNAL).join(staging_name);
    let substituted_now = directory_inventory(&substituted);
    assert_eq!(
        &substituted_now, substituted_inventory,
        "the substituted copy keeps its own exact copied-entry identities and byte manifest"
    );
    // Cross-copy coherence is content-only and deliberately excludes identity:
    // the copied entries carry the staged bytes with distinct inodes.
    assert_eq!(
        content_projection(&moved_now),
        content_projection(&substituted_now),
        "the copy is byte-coherent with the verified staging content"
    );
    #[cfg(unix)]
    {
        assert_eq!(
            &directory_identity(&substituted),
            substituted_identity,
            "the exact substituted directory observed at intervention is still in place"
        );
        assert_ne!(
            &directory_identity(&substituted),
            staging_identity,
            "the substituted directory is a different identity"
        );
    }
    assert_whole_release(&root, "1.0.0");
}

#[test]
fn a_tampered_promoted_generation_is_never_activated() {
    // BeforeActivate: the tamper lands after the pre-activation observation
    // but before activation; the post-boundary verification must refuse.
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let first_installer = installer(root_holder.path());
    install(&first_installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let before = current_target(&root);
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(FailPoint::BeforeActivate, move |_| {
        let internal = hook_root.join(INTERNAL);
        let journal =
            std::fs::read_to_string(internal.join("journal.json")).map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_str(&journal).map_err(|_| InstallError::Io)?;
        let generation = value["generation"].as_str().ok_or(InstallError::Io)?;
        std::fs::write(
            hook_root
                .join("releases")
                .join(generation)
                .join("kontor-mcp"),
            b"tampered before activation",
        )
        .map_err(|_| InstallError::Io)
    })]);
    let error = first_installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a BeforeActivate tamper is never activated");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert_eq!(current_target(&root), before);
    assert!(root.join(INTERNAL).join("journal.json").exists());

    // AfterActivationTemp: the activation effect happens after the tamper, so
    // only the last pre-effect observation inside activation can catch it; it
    // must, and no success may follow.
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let before = current_target(&root);
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(FailPoint::AfterActivationTemp, move |_| {
        let internal = hook_root.join(INTERNAL);
        let journal =
            std::fs::read_to_string(internal.join("journal.json")).map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_str(&journal).map_err(|_| InstallError::Io)?;
        let generation = value["generation"].as_str().ok_or(InstallError::Io)?;
        std::fs::write(
            hook_root
                .join("releases")
                .join(generation)
                .join("kontor-mcp"),
            b"tampered at activation temporary",
        )
        .map_err(|_| InstallError::Io)
    })]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("an AfterActivationTemp tamper is never a success");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert_eq!(
        current_target(&root),
        before,
        "the old pointer is unchanged: the tamper is caught before activation"
    );
    assert!(root.join(INTERNAL).join("journal.json").exists());
}

#[test]
fn a_coherent_marker_and_artifact_tamper_before_finalize_is_refused() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(FailPoint::BeforeFinalize, move |_| {
        let internal = hook_root.join(INTERNAL);
        let journal =
            std::fs::read_to_string(internal.join("journal.json")).map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_str(&journal).map_err(|_| InstallError::Io)?;
        let generation = value["generation"].as_str().ok_or(InstallError::Io)?;
        let directory = hook_root.join("releases").join(generation);
        let tampered = b"coherently tampered bytes";
        std::fs::write(directory.join("kontor-mcp"), tampered).map_err(|_| InstallError::Io)?;
        // Update the marker so the generation is internally coherent; the
        // journal manifest must still refuse it.
        let marker_path = directory.join(".kontor-generation.json");
        let mut marker: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&marker_path).map_err(|_| InstallError::Io)?,
        )
        .map_err(|_| InstallError::Io)?;
        let digest = sha256_hex(tampered);
        if let Some(artifacts) = marker["artifacts"].as_array_mut() {
            for artifact in artifacts {
                if artifact["name"] == "mcp" {
                    artifact["sha256"] = serde_json::json!(digest);
                }
            }
        }
        std::fs::write(
            &marker_path,
            serde_json::to_vec(&marker).map_err(|_| InstallError::Io)?,
        )
        .map_err(|_| InstallError::Io)
    })]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a coherent marker tamper must still refuse");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert!(root.join(INTERNAL).join("journal.json").exists());
}

#[cfg(unix)]
#[test]
fn recovery_post_activation_retarget_is_preserved() {
    use std::os::unix::fs::symlink;
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "promoted", &version.manifest, "123-456");
    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
    write_generation(root_holder.path(), "1.0.0-999-999", &version.manifest);
    symlink("releases/1.0.0-999-999", root_holder.path().join("current"))
        .expect("retargeted pointer");
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        current_target(root_holder.path()).as_deref(),
        Some("releases/1.0.0-999-999")
    );
    assert!(
        root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

#[test]
fn an_activation_temporary_retargeted_after_the_boundary_is_never_activated() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let root_holder = tempfile::tempdir().expect("root");
        let version_one = release("1.0.0");
        let version_two = release("2.0.0");
        let installer = installer(root_holder.path());
        install(&installer, &version_one).expect("baseline");
        let root = root_holder.path().to_path_buf();
        let before = current_target(&root);
        let hook_root = root.clone();
        let mut hooks = Hooks(vec![hook(FailPoint::AfterActivationTemp, move |_| {
            let temporary = std::fs::read_dir(&hook_root)
                .map_err(|_| InstallError::Io)?
                .filter_map(Result::ok)
                .find(|entry| entry.file_name().to_string_lossy().starts_with(".current-"))
                .ok_or(InstallError::Io)?;
            std::fs::remove_file(temporary.path()).map_err(|_| InstallError::Io)?;
            symlink("releases/1.0.0-999-999", temporary.path()).map_err(|_| InstallError::Io)
        })]);
        let error = installer
            .install(
                &version_two.manifest,
                &version_two.sources,
                &version_two.dir(),
                &mut hooks,
            )
            .expect_err("a retargeted activation temporary is never activated");
        assert_eq!(error, InstallError::ActiveUnverified);
        assert_eq!(current_target(&root), before);
        assert!(root.join(INTERNAL).join("journal.json").exists());
    }
}

#[test]
fn a_deleted_current_pointer_is_never_clean_install_admission() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let baseline_generation = current_target(&root).expect("baseline pointer");
    let baseline_inventory = directory_inventory(&root.join(&baseline_generation));
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(FailPoint::BeforeActivate, move |_| {
        std::fs::remove_file(hook_root.join("current")).map_err(|_| InstallError::Io)
    })]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("an absent expected-previous pointer is a change, not admission");
    assert_eq!(error, InstallError::ActivationChanged);
    assert_eq!(
        pointer_observation(&root),
        "absent",
        "the exact current name is absent by symlink_metadata NotFound"
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&journal_bytes(&root)).expect("journal json");
    assert_eq!(journal["state"], "promoted");
    assert_eq!(journal["previous"]["release"], "1.0.0");
    assert_eq!(
        journal["previous"]["generation"],
        baseline_generation
            .strip_prefix("releases/")
            .expect("generation shape")
    );
    // The old release is verified directly through its retained
    // baseline_generation path; no active pointer is required after the
    // current name was intentionally removed by the intervention.
    assert_eq!(
        directory_inventory(&root.join(&baseline_generation)),
        baseline_inventory,
        "the retained old generation's complete inventory is unchanged"
    );
}

#[test]
fn recovery_post_activation_tamper_is_preserved_for_every_branch() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        // Three admissible recovery setups: staged-present, promoted-before-
        // journal (staging missing), and journal-Promoted. Each performs a
        // real activation, then the AfterSwap hook retargets the pointer; the
        // post-activation verification must preserve everything.
        enum Setup {
            StagedPresent,
            PromotedBeforeJournal,
            Promoted,
        }
        for setup in [
            Setup::StagedPresent,
            Setup::PromotedBeforeJournal,
            Setup::Promoted,
        ] {
            let root_holder = tempfile::tempdir().expect("root");
            let version = release("1.0.0");
            write_generation(root_holder.path(), "1.0.0-999-999", &version.manifest);
            match setup {
                Setup::StagedPresent => {
                    write_journal(root_holder.path(), "staged", &version.manifest, "123-456");
                    let staging = root_holder.path().join(INTERNAL).join("staging-123-456");
                    std::fs::create_dir_all(&staging).expect("staging");
                    write_staging_owner(&staging, "123-456", &version.manifest);
                    for name in ArtifactName::ALL {
                        std::fs::write(
                            staging.join(name.file_name()),
                            format!("{}@1.0.0", name.file_name()),
                        )
                        .expect("artifact");
                    }
                    std::fs::write(
                        staging.join(".kontor-generation.json"),
                        serde_json::to_vec(&serde_json::json!({
                            "schema_version": 1,
                            "generation": "1.0.0-123-456",
                            "release": "1.0.0",
                            "nonce": "123-456",
                            "artifacts": version.manifest.artifacts,
                        }))
                        .expect("marker bytes"),
                    )
                    .expect("marker");
                }
                Setup::PromotedBeforeJournal => {
                    write_journal(root_holder.path(), "staged", &version.manifest, "123-456");
                    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
                }
                Setup::Promoted => {
                    write_journal(root_holder.path(), "promoted", &version.manifest, "123-456");
                    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
                }
            }
            let root = root_holder.path().to_path_buf();
            let hook_root = root.clone();
            let mut faults = Hooks(vec![hook(
                FailPoint::AfterSwap(ArtifactName::Mcp),
                move |_| {
                    let pointer = hook_root.join("current");
                    std::fs::remove_file(&pointer).map_err(|_| InstallError::Io)?;
                    symlink("releases/1.0.0-999-999", &pointer).map_err(|_| InstallError::Io)
                },
            )]);
            let installer = installer(&root);
            assert_eq!(
                installer.recover(&mut faults),
                Ok(RecoveryOutcome::PreservedConflict)
            );
            assert_eq!(
                current_target(&root).as_deref(),
                Some("releases/1.0.0-999-999"),
                "the injected retarget is preserved as evidence"
            );
            assert!(root.join(INTERNAL).join("journal.json").exists());
        }
    }
}

#[test]
fn a_valid_journal_naming_foreign_staging_is_preserved() {
    let root_holder = tempfile::tempdir().expect("root");
    let installer = installer(root_holder.path());
    let version = release("1.0.0");
    let manifest_value = serde_json::to_value(&version.manifest).expect("manifest");
    let internal = root_holder.path().join(INTERNAL);
    let staging = internal.join("staging-123-456");
    std::fs::create_dir_all(&staging).expect("staging");
    std::fs::write(staging.join("foreign.bin"), "foreign").expect("foreign");
    std::fs::write(
        internal.join("journal.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "nonce": "123-456",
            "generation": "1.0.0-123-456",
            "state": "prepared",
            "manifest": manifest_value,
            "previous": null,
        }))
        .expect("journal"),
    )
    .expect("write");

    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        std::fs::read_to_string(staging.join("foreign.bin")).expect("foreign read"),
        "foreign"
    );
    assert!(internal.join("journal.json").exists());
}

#[test]
fn a_foreign_active_pointer_is_never_overwritten() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let root_holder = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let version_one = release("1.0.0");
        let version_two = release("2.0.0");
        let installer = installer(root_holder.path());
        install(&installer, &version_one).expect("baseline");

        let root = root_holder.path().to_path_buf();
        let swap_root = root.clone();
        let swap_outside = outside.path().to_path_buf();
        let mut hooks = Hooks(vec![hook(
            FailPoint::BeforeSwap(ArtifactName::Mcp),
            move |_| {
                std::fs::remove_file(swap_root.join("current")).map_err(|_| InstallError::Io)?;
                symlink(&swap_outside, swap_root.join("current")).map_err(|_| InstallError::Io)
            },
        )]);
        let error = installer
            .install(
                &version_two.manifest,
                &version_two.sources,
                &version_two.dir(),
                &mut hooks,
            )
            .expect_err("foreign pointer is preserved");
        // Source contract: the transaction observed the valid previous pointer
        // when it began; the intervention then changed `current` to a foreign
        // target before activation, so `activate` refuses the changed pointer
        // as ActivationChanged (existing != expected_previous) and never
        // reaches read_active's foreign-pointer ActiveUnverified refusal.
        assert_eq!(error, InstallError::ActivationChanged);
        assert_eq!(
            std::fs::read_link(root.join("current")).expect("pointer"),
            outside.path()
        );
        let preserved = std::fs::read_dir(root.join("releases"))
            .expect("releases")
            .filter_map(Result::ok)
            .any(|generation| {
                std::fs::read_to_string(generation.path().join("kontor"))
                    .is_ok_and(|text| text == "kontor@1.0.0")
            });
        assert!(preserved, "the previous generation is still on disk");
        assert_eq!(
            installer.recover(&mut NoFaults),
            Ok(RecoveryOutcome::PreservedConflict)
        );
        assert_eq!(
            std::fs::read_link(root.join("current")).expect("pointer"),
            outside.path()
        );
    }
}

fn current_target(root: &Path) -> Option<String> {
    std::fs::read_link(root.join("current"))
        .ok()
        .map(|target| target.to_string_lossy().into_owned())
}

/// Exact current-pointer observation: absence is asserted through
/// `symlink_metadata` NotFound, and a read or kind error panics instead of
/// collapsing to `None`. A present pointer must be a symlink, and its target
/// and own identity are included in the observation string.
fn pointer_observation(root: &Path) -> String {
    let path = root.join("current");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "absent".to_owned(),
        Err(error) => panic!("pointer observation error for {}: {error}", path.display()),
        Ok(metadata) => {
            assert!(
                metadata.file_type().is_symlink(),
                "the current name must be a symlink when present"
            );
            let target = std::fs::read_link(&path).expect("pointer target");
            format!("symlink:{}:{}", target.display(), symlink_identity(&path))
        }
    }
}

#[cfg(unix)]
fn symlink_identity(path: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path).expect("symlink metadata");
    format!("{}:{}", metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn symlink_identity(_path: &Path) -> String {
    "no-identity".to_owned()
}

/// A directory's exact dev/inode identity as a comparable string.
#[cfg(unix)]
fn directory_identity(path: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).expect("directory metadata");
    format!("{}:{}", metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn directory_identity(_path: &Path) -> String {
    "no-identity".to_owned()
}

/// Exact bytes of one file; a read error panics rather than reading as absent.
fn file_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("file bytes for {}: {error}", path.display()))
}

/// A sorted exact inventory of one directory: name, kind, content hash and
/// per-entry identity (dev:inode on unix, empty otherwise). A read, kind,
/// metadata or entry error panics; nothing collapses to a silent absence.
/// FIFOs are typed without reading, because a blocking open is not a
/// deterministic observation. The inventory is direct-level only: it does not
/// recurse into child directories.
fn directory_inventory(path: &Path) -> Vec<(String, String, String, String)> {
    let mut entries: Vec<(String, String, String, String)> = std::fs::read_dir(path)
        .unwrap_or_else(|error| panic!("inventory for {}: {error}", path.display()))
        .map(|entry| {
            let entry = entry.expect("entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            let file_type = entry.file_type().expect("file type");
            let kind = if file_type.is_dir() {
                "dir".to_owned()
            } else if file_type.is_symlink() {
                "symlink".to_owned()
            } else if is_fifo(&file_type) {
                "fifo".to_owned()
            } else {
                "file".to_owned()
            };
            let hash = if kind == "file" {
                sha256_hex(&std::fs::read(entry.path()).expect("entry bytes"))
            } else {
                String::new()
            };
            (name, kind, hash, entry_identity(&entry.path()))
        })
        .collect();
    entries.sort();
    entries
}

#[cfg(unix)]
fn entry_identity(path: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path).expect("entry identity metadata");
    format!("{}:{}", metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn entry_identity(_path: &Path) -> String {
    String::new()
}

/// The content-only projection of an inventory: name, kind and whole-file
/// content hash, deliberately excluding per-entry identity. It is used only
/// for cross-copy coherence checks; identity preservation is asserted against
/// each directory's own retained inventory.
fn content_projection(
    inventory: &[(String, String, String, String)],
) -> Vec<(String, String, String)> {
    inventory
        .iter()
        .map(|(name, kind, hash, _)| (name.clone(), kind.clone(), hash.clone()))
        .collect()
}

#[cfg(unix)]
fn is_fifo(file_type: &std::fs::FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;
    file_type.is_fifo()
}

#[cfg(not(unix))]
fn is_fifo(_file_type: &std::fs::FileType) -> bool {
    false
}

/// Exact journal bytes; a missing or unreadable journal panics.
fn journal_bytes(root: &Path) -> Vec<u8> {
    file_bytes(&root.join(INTERNAL).join("journal.json"))
}

fn write_generation(root: &Path, generation: &str, manifest: &ArtifactManifest) {
    let directory = root.join("releases").join(generation);
    std::fs::create_dir_all(&directory).expect("generation");
    for name in ArtifactName::ALL {
        std::fs::write(
            directory.join(name.file_name()),
            format!("{}@{}", name.file_name(), manifest.release),
        )
        .expect("artifact");
    }
    let nonce = generation
        .strip_prefix(&format!("{}-", manifest.release))
        .unwrap_or(generation);
    std::fs::write(
        directory.join(".kontor-generation.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "generation": generation,
            "release": manifest.release,
            "nonce": nonce,
            "artifacts": manifest.artifacts,
        }))
        .expect("marker bytes"),
    )
    .expect("marker");
}

#[test]
fn a_staged_artifact_tampered_after_staging_is_never_promoted_or_activated() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(
        FailPoint::BeforeSwap(ArtifactName::Mcp),
        move |_| {
            let internal = hook_root.join(INTERNAL);
            let staging = std::fs::read_dir(&internal)
                .map_err(|_| InstallError::Io)?
                .filter_map(Result::ok)
                .find(|entry| entry.file_name().to_string_lossy().starts_with("staging-"))
                .ok_or(InstallError::Io)?;
            std::fs::write(staging.path().join("kontor-mcp"), b"tampered after staging")
                .map_err(|_| InstallError::Io)
        },
    )]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a tampered staging is never promoted");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert_eq!(
        std::fs::read_dir(root.join("releases"))
            .expect("releases")
            .count(),
        1,
        "no new generation appears"
    );
    assert_whole_release(&root, "1.0.0");
}

#[test]
fn a_promoted_generation_tampered_before_activation_is_never_activated() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let before = current_target(&root);
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(FailPoint::AfterPromote, move |_| {
        let internal = hook_root.join(INTERNAL);
        let journal =
            std::fs::read_to_string(internal.join("journal.json")).map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_str(&journal).map_err(|_| InstallError::Io)?;
        let generation = value["generation"].as_str().ok_or(InstallError::Io)?;
        std::fs::write(
            hook_root
                .join("releases")
                .join(generation)
                .join("kontor-mcp"),
            b"tampered after promotion",
        )
        .map_err(|_| InstallError::Io)
    })]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a tampered promotion is never activated");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert_eq!(current_target(&root), before);
    assert!(root.join(INTERNAL).join("journal.json").exists());
}

#[test]
fn an_activated_generation_is_reverified_before_success() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(
        FailPoint::AfterSwap(ArtifactName::Mcp),
        move |_| {
            let internal = hook_root.join(INTERNAL);
            let journal = std::fs::read_to_string(internal.join("journal.json"))
                .map_err(|_| InstallError::Io)?;
            let value: serde_json::Value =
                serde_json::from_str(&journal).map_err(|_| InstallError::Io)?;
            let generation = value["generation"].as_str().ok_or(InstallError::Io)?;
            std::fs::write(
                hook_root
                    .join("releases")
                    .join(generation)
                    .join("kontor-mcp"),
                b"tampered after activation",
            )
            .map_err(|_| InstallError::Io)
        },
    )]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a tampered active generation is never a success");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert!(root.join(INTERNAL).join("journal.json").exists());
    assert_eq!(
        installer
            .status(&version_one.manifest)
            .expect_err("unverifiable"),
        InstallError::ActiveUnverified
    );
}

#[test]
fn recovery_completes_a_generation_promoted_before_the_journal_update() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "staged", &version.manifest, "123-456");
    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::Completed)
    );
    assert_eq!(
        current_target(root_holder.path()).as_deref(),
        Some("releases/1.0.0-123-456")
    );
    assert_whole_release(root_holder.path(), "1.0.0");
    assert!(
        !root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn recovery_reclaims_only_its_own_activation_temporary() {
    use std::os::unix::fs::symlink;
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "promoted", &version.manifest, "123-456");
    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
    symlink(
        "releases/1.0.0-123-456",
        root_holder.path().join(".current-123-456.tmp"),
    )
    .expect("activation temp");
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::Completed)
    );
    assert!(!root_holder.path().join(".current-123-456.tmp").exists());
    assert_eq!(
        current_target(root_holder.path()).as_deref(),
        Some("releases/1.0.0-123-456")
    );
}

#[cfg(unix)]
#[test]
fn a_retargeted_activation_temporary_is_preserved_with_recovery_required() {
    use std::os::unix::fs::symlink;
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "promoted", &version.manifest, "123-456");
    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
    let temporary = root_holder.path().join(".current-123-456.tmp");
    symlink("releases/1.0.0-999-999", &temporary).expect("retargeted temp");
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert!(std::fs::symlink_metadata(&temporary).is_ok());
    assert_eq!(
        std::fs::read_link(&temporary).expect("readlink"),
        Path::new("releases/1.0.0-999-999")
    );
    assert!(
        root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
    assert_eq!(current_target(root_holder.path()), None);
}

#[test]
fn an_incoherent_version_token_is_not_reflected() {
    let version = release("1.0.0");
    let mut document = serde_json::to_value(&version.manifest).expect("manifest json");
    document["artifacts"][0]["version"] = serde_json::json!("SYNTHETIC_SECRET");
    let parsed = ArtifactManifest::parse(&serde_json::to_string(&document).expect("json"));
    let error = parsed.expect_err("an incoherent version is refused");
    assert_eq!(
        error,
        ManifestError::IncoherentVersion {
            name: ArtifactName::Cli
        }
    );
    assert!(
        !error.to_string().contains("SYNTHETIC_SECRET"),
        "the supplied token is not reflected: {error}"
    );
}

#[test]
fn an_equal_digest_different_release_is_not_already_current() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    // Identical bytes and digests under a different release are a new
    // coherent install, not an already-current state.
    let same_digests = ArtifactManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        release: "2.0.0".to_owned(),
        artifacts: version_one
            .manifest
            .artifacts
            .iter()
            .map(|entry| ArtifactEntry {
                name: entry.name,
                version: "2.0.0".to_owned(),
                sha256: entry.sha256.clone(),
            })
            .collect(),
    };
    let sources = same_digests
        .verify_sources(&version_one.directory)
        .expect("verified against the same bytes");
    assert_eq!(
        installer.install(&same_digests, &sources, &version_one.dir(), &mut NoFaults),
        Ok(InstallOutcome::Updated)
    );
    let marker = std::fs::read_to_string(
        root_holder
            .path()
            .join("current")
            .join(".kontor-generation.json"),
    )
    .expect("marker");
    assert!(marker.contains("\"release\":\"2.0.0\""), "{marker}");
    assert_eq!(
        installer.install(&same_digests, &sources, &version_one.dir(), &mut NoFaults),
        Ok(InstallOutcome::AlreadyCurrent)
    );
}

#[cfg(unix)]
#[test]
fn a_swapped_manifest_path_is_refused() {
    use std::os::unix::fs::symlink;
    let holder = tempfile::tempdir().expect("holder");
    let version = release("1.0.0");
    let manifest = holder.path().join("manifest.json");
    std::fs::write(
        &manifest,
        serde_json::to_vec(&version.manifest).expect("json"),
    )
    .expect("manifest");
    let elsewhere = holder.path().join("elsewhere.json");
    std::fs::write(&elsewhere, b"{}").expect("elsewhere");
    std::fs::remove_file(&manifest).expect("remove");
    symlink(&elsewhere, &manifest).expect("swap");
    assert_eq!(
        ArtifactManifest::read(&manifest),
        Err(ManifestError::InvalidDocument)
    );
}

#[test]
fn a_generation_created_after_the_check_is_never_replaced() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let swap_root = root.clone();
    let mut hooks = Hooks(vec![hook(
        FailPoint::BeforeSwap(ArtifactName::Mcp),
        move |_| {
            let internal = swap_root.join(INTERNAL);
            let staging = std::fs::read_dir(&internal)
                .map_err(|_| InstallError::Io)?
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .find(|name| name.starts_with("staging-"))
                .ok_or(InstallError::Io)?;
            let generation = format!("2.0.0-{}", staging.trim_start_matches("staging-"));
            std::fs::create_dir(swap_root.join("releases").join(generation))
                .map_err(|_| InstallError::Io)
        },
    )]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a generation created after the check is never replaced");
    assert_eq!(error, InstallError::GenerationCollision);
    assert_whole_release(root_holder.path(), "1.0.0");
    let foreign_empty = std::fs::read_dir(root.join("releases"))
        .expect("releases")
        .filter_map(Result::ok)
        .any(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && std::fs::read_dir(entry.path())
                    .map(|mut entries| entries.next().is_none())
                    .unwrap_or(false)
        });
    assert!(
        foreign_empty,
        "the foreign generation directory is preserved"
    );
}

#[test]
fn a_foreign_journal_temporary_is_preserved() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(
        FailPoint::AfterStage(ArtifactName::Bootstrap),
        move |_| {
            let internal = hook_root.join(INTERNAL);
            let journal = std::fs::read_to_string(internal.join("journal.json"))
                .map_err(|_| InstallError::Io)?;
            let value: serde_json::Value =
                serde_json::from_str(&journal).map_err(|_| InstallError::Io)?;
            let nonce = value["nonce"].as_str().ok_or(InstallError::Io)?;
            std::fs::write(
                internal.join(format!("journal.json.tmp-{nonce}")),
                b"foreign temp",
            )
            .map_err(|_| InstallError::Io)
        },
    )]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a foreign journal temporary is never removed");
    assert_eq!(error, InstallError::TemporaryTaken);
    assert_whole_release(root_holder.path(), "1.0.0");
    let internal = root.join(INTERNAL);
    let foreign = std::fs::read_dir(&internal)
        .expect("internal")
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("journal.json.tmp-")
        })
        .expect("the foreign temporary is preserved");
    assert_eq!(
        std::fs::read_to_string(foreign.path()).expect("read"),
        "foreign temp"
    );
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::Nothing)
    );
    assert!(foreign.path().exists());
}

#[test]
fn a_directory_swap_cannot_redirect_promotion() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let root_holder = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let version_one = release("1.0.0");
        let version_two = release("2.0.0");
        let installer = installer(root_holder.path());
        install(&installer, &version_one).expect("baseline");

        let moved_holder = tempfile::tempdir().expect("moved holder");
        let root = root_holder.path().to_path_buf();
        let moved = moved_holder.path().join("root-moved");
        let swap_root = root.clone();
        let swap_moved = moved.clone();
        let swap_outside = outside.path().to_path_buf();
        let mut hooks = Hooks(vec![hook(
            FailPoint::BeforeSwap(ArtifactName::Mcp),
            move |_| {
                std::fs::rename(&swap_root, &swap_moved).map_err(|_| InstallError::Io)?;
                symlink(&swap_outside, &swap_root).map_err(|_| InstallError::Io)
            },
        )]);
        let outcome = installer
            .install(
                &version_two.manifest,
                &version_two.sources,
                &version_two.dir(),
                &mut hooks,
            )
            .expect("the held descriptor writes into the moved directory");
        assert_eq!(outcome, InstallOutcome::Updated);
        assert!(
            std::fs::read_dir(outside.path())
                .expect("outside")
                .next()
                .is_none(),
            "nothing may land outside the held root"
        );
        let moved_pointer = std::fs::read_link(moved.join("current")).expect("moved pointer");
        let moved_generation = moved.join(moved_pointer);
        assert_eq!(
            std::fs::read_to_string(moved_generation.join("kontor")).expect("artifact"),
            "kontor@2.0.0"
        );
    }
}

#[test]
fn only_one_bootstrap_can_transact_on_a_root() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let mut faults = ScriptedFaults::observing(FailPoint::BeforeStage, |root| {
        let path = root.join(INTERNAL).join("lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|_| InstallError::Io)?;
        match fs4::FileExt::try_lock(&file) {
            Err(fs4::TryLockError::WouldBlock) => Ok(()),
            _ => Err(InstallError::Locked),
        }
    });
    assert_eq!(
        installer.install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut faults
        ),
        Ok(InstallOutcome::Updated)
    );
}

#[test]
fn a_concurrent_writer_during_rollback_is_never_removed() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let root_holder = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let version = release("1.0.0");
        let installer = installer(root_holder.path());
        let internal = root_holder.path().join(INTERNAL);
        let staging = internal.join("staging-123-456");
        std::fs::create_dir_all(&staging).expect("staging");
        std::fs::write(staging.join("foreign.bin"), "foreign").expect("foreign");
        let manifest_value = serde_json::to_value(&version.manifest).expect("manifest");
        std::fs::write(
            internal.join("journal.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 2,
                "nonce": "123-456",
                "generation": "1.0.0-123-456",
                "state": "staged",
                "manifest": manifest_value,
                "previous": null,
            }))
            .expect("journal"),
        )
        .expect("write");
        // A foreign current pointer plus foreign staging: recovery must touch
        // neither.
        symlink(outside.path(), root_holder.path().join("current")).expect("pointer");
        assert_eq!(
            installer.recover(&mut NoFaults),
            Ok(RecoveryOutcome::PreservedConflict)
        );
        assert_eq!(
            std::fs::read_to_string(staging.join("foreign.bin")).expect("foreign"),
            "foreign"
        );
        assert_eq!(
            std::fs::read_link(root_holder.path().join("current")).expect("pointer"),
            outside.path()
        );
    }
}

/// Replace the promoted generation's `kontor-mcp` bytes and rewrite the marker
/// so the generation is internally coherent; only the journal binding can
/// refuse it.
fn coherently_tamper_generation(root: &Path, bytes: &[u8]) -> Result<(), InstallError> {
    let internal = root.join(INTERNAL);
    let journal =
        std::fs::read_to_string(internal.join("journal.json")).map_err(|_| InstallError::Io)?;
    let value: serde_json::Value = serde_json::from_str(&journal).map_err(|_| InstallError::Io)?;
    let generation = value["generation"].as_str().ok_or(InstallError::Io)?;
    let directory = root.join("releases").join(generation);
    std::fs::write(directory.join("kontor-mcp"), bytes).map_err(|_| InstallError::Io)?;
    let marker_path = directory.join(".kontor-generation.json");
    let mut marker: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&marker_path).map_err(|_| InstallError::Io)?)
            .map_err(|_| InstallError::Io)?;
    let digest = sha256_hex(bytes);
    if let Some(artifacts) = marker["artifacts"].as_array_mut() {
        for artifact in artifacts {
            if artifact["name"] == "mcp" {
                artifact["sha256"] = serde_json::json!(digest);
            }
        }
    }
    std::fs::write(
        &marker_path,
        serde_json::to_vec(&marker).map_err(|_| InstallError::Io)?,
    )
    .map_err(|_| InstallError::Io)
}

/// An owned, complete staging directory for recovery fixtures.
fn write_owned_staging(
    root: &Path,
    manifest: &ArtifactManifest,
    nonce: &str,
) -> std::path::PathBuf {
    let staging = root.join(INTERNAL).join(format!("staging-{nonce}"));
    std::fs::create_dir_all(&staging).expect("staging");
    write_staging_owner(&staging, nonce, manifest);
    for name in ArtifactName::ALL {
        std::fs::write(
            staging.join(name.file_name()),
            format!("{}@{}", name.file_name(), manifest.release),
        )
        .expect("artifact");
    }
    staging
}

#[test]
fn a_clean_install_coherent_marker_tamper_after_activation_temp_leaves_no_pointer() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    let installer = installer(root_holder.path());
    let root = root_holder.path().to_path_buf();
    let hook_root = root.clone();
    let journal_before: std::rc::Rc<std::cell::RefCell<Option<Vec<u8>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let generation_cell: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_journal = std::rc::Rc::clone(&journal_before);
    let hook_generation = std::rc::Rc::clone(&generation_cell);
    let mut hooks = Hooks(vec![hook(FailPoint::AfterActivationTemp, move |_| {
        // The last pre-effect boundary: the promotion completed and the
        // pre-activation verification passed; only the post-boundary
        // generation binding can refuse this coherent tamper.
        let journal = std::fs::read(hook_root.join(INTERNAL).join("journal.json"))
            .map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_slice(&journal).map_err(|_| InstallError::Io)?;
        let generation = value["generation"]
            .as_str()
            .ok_or(InstallError::Io)?
            .to_owned();
        *hook_journal.borrow_mut() = Some(journal);
        *hook_generation.borrow_mut() = Some(generation);
        coherently_tamper_generation(&hook_root, b"clean install coherent tamper")
    })]);
    let error = installer
        .install(
            &version.manifest,
            &version.sources,
            &version.dir(),
            &mut hooks,
        )
        .expect_err("a clean-install coherent tamper is never activated");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert_eq!(
        pointer_observation(&root),
        "absent",
        "a refused clean install leaves the pointer absent by exact NotFound"
    );
    assert_eq!(
        journal_bytes(&root),
        journal_before.borrow().clone().expect("captured journal"),
        "the exact journal bytes are retained"
    );
    let generation = generation_cell.borrow().clone().expect("generation");
    assert_eq!(
        file_bytes(&root.join("releases").join(&generation).join("kontor-mcp")),
        b"clean install coherent tamper",
        "the complete tampered bytes are preserved as evidence"
    );
}

#[test]
fn a_clean_install_content_tamper_after_activation_temp_leaves_no_pointer() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    let installer = installer(root_holder.path());
    let root = root_holder.path().to_path_buf();
    let hook_root = root.clone();
    let journal_before: std::rc::Rc<std::cell::RefCell<Option<Vec<u8>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let generation_cell: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_journal = std::rc::Rc::clone(&journal_before);
    let hook_generation = std::rc::Rc::clone(&generation_cell);
    let mut hooks = Hooks(vec![hook(FailPoint::AfterActivationTemp, move |_| {
        let journal = std::fs::read(hook_root.join(INTERNAL).join("journal.json"))
            .map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_slice(&journal).map_err(|_| InstallError::Io)?;
        let generation = value["generation"]
            .as_str()
            .ok_or(InstallError::Io)?
            .to_owned();
        *hook_journal.borrow_mut() = Some(journal);
        *hook_generation.borrow_mut() = Some(generation.clone());
        std::fs::write(
            hook_root
                .join("releases")
                .join(&generation)
                .join("kontor-mcp"),
            b"clean install content tamper",
        )
        .map_err(|_| InstallError::Io)
    })]);
    let error = installer
        .install(
            &version.manifest,
            &version.sources,
            &version.dir(),
            &mut hooks,
        )
        .expect_err("a content tamper is never activated");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert_eq!(pointer_observation(&root), "absent");
    assert_eq!(
        journal_bytes(&root),
        journal_before.borrow().clone().expect("captured journal")
    );
    let generation = generation_cell.borrow().clone().expect("generation");
    assert_eq!(
        file_bytes(&root.join("releases").join(&generation).join("kontor-mcp")),
        b"clean install content tamper"
    );
}

#[test]
fn a_clean_install_content_tamper_after_swap_is_refused_with_exact_pointer_evidence() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    let installer = installer(root_holder.path());
    let root = root_holder.path().to_path_buf();
    let hook_root = root.clone();
    let journal_before: std::rc::Rc<std::cell::RefCell<Option<Vec<u8>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let generation_cell: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_journal = std::rc::Rc::clone(&journal_before);
    let hook_generation = std::rc::Rc::clone(&generation_cell);
    let mut hooks = Hooks(vec![hook(
        FailPoint::AfterSwap(ArtifactName::Mcp),
        move |_| {
            let journal = std::fs::read(hook_root.join(INTERNAL).join("journal.json"))
                .map_err(|_| InstallError::Io)?;
            let value: serde_json::Value =
                serde_json::from_slice(&journal).map_err(|_| InstallError::Io)?;
            let generation = value["generation"]
                .as_str()
                .ok_or(InstallError::Io)?
                .to_owned();
            *hook_journal.borrow_mut() = Some(journal);
            *hook_generation.borrow_mut() = Some(generation.clone());
            std::fs::write(
                hook_root
                    .join("releases")
                    .join(&generation)
                    .join("kontor-mcp"),
                b"clean install after swap tamper",
            )
            .map_err(|_| InstallError::Io)
        },
    )]);
    let error = installer
        .install(
            &version.manifest,
            &version.sources,
            &version.dir(),
            &mut hooks,
        )
        .expect_err("a clean-install after-swap tamper is never a success");
    assert_eq!(error, InstallError::ActiveUnverified);
    let generation = generation_cell.borrow().clone().expect("generation");
    // The activation effect occurred, so the pointer names exactly the
    // tampered generation; the failure is the final active verification, and
    // no success or completion is reported.
    let pointer =
        std::fs::symlink_metadata(root.join("current")).expect("the activated pointer exists");
    assert!(pointer.file_type().is_symlink());
    assert_eq!(
        std::fs::read_link(root.join("current")).expect("pointer target"),
        PathBuf::from(format!("releases/{generation}"))
    );
    assert_eq!(
        file_bytes(&root.join("releases").join(&generation).join("kontor-mcp")),
        b"clean install after swap tamper"
    );
    assert_eq!(
        journal_bytes(&root),
        journal_before.borrow().clone().expect("captured journal")
    );
}

#[test]
fn an_after_activation_temp_coherent_marker_tamper_is_never_activated() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let before = pointer_observation(&root);
    let hook_root = root.clone();
    let journal_before: std::rc::Rc<std::cell::RefCell<Option<Vec<u8>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let generation_cell: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_journal = std::rc::Rc::clone(&journal_before);
    let hook_generation = std::rc::Rc::clone(&generation_cell);
    let mut hooks = Hooks(vec![hook(FailPoint::AfterActivationTemp, move |_| {
        let journal = std::fs::read(hook_root.join(INTERNAL).join("journal.json"))
            .map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_slice(&journal).map_err(|_| InstallError::Io)?;
        let generation = value["generation"]
            .as_str()
            .ok_or(InstallError::Io)?
            .to_owned();
        *hook_journal.borrow_mut() = Some(journal);
        *hook_generation.borrow_mut() = Some(generation);
        coherently_tamper_generation(&hook_root, b"coherent after activation temporary")
    })]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a coherent marker tamper is never activated");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert_eq!(
        pointer_observation(&root),
        before,
        "the old pointer kind/identity/target is unchanged; the journal binding refuses the tamper"
    );
    assert_eq!(
        journal_bytes(&root),
        journal_before.borrow().clone().expect("captured journal")
    );
    let generation = generation_cell.borrow().clone().expect("generation");
    assert_eq!(
        file_bytes(&root.join("releases").join(&generation).join("kontor-mcp")),
        b"coherent after activation temporary"
    );
}

#[test]
fn recovery_refuses_a_content_tampered_promoted_generation() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "promoted", &version.manifest, "123-456");
    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
    let tampered = b"tampered before recovery activation";
    std::fs::write(
        root_holder.path().join("releases/1.0.0-123-456/kontor-mcp"),
        tampered,
    )
    .expect("tamper");
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(current_target(root_holder.path()), None);
    assert_eq!(
        std::fs::read(root_holder.path().join("releases/1.0.0-123-456/kontor-mcp")).expect("read"),
        tampered.to_vec(),
        "the tampered bytes are preserved as evidence"
    );
    assert!(
        root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

#[test]
fn recovery_refuses_a_coherently_marked_tampered_generation() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "promoted", &version.manifest, "123-456");
    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
    let tampered = b"coherently tampered before recovery";
    coherently_tamper_generation(root_holder.path(), tampered).expect("tamper");
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(current_target(root_holder.path()), None);
    assert_eq!(
        std::fs::read(root_holder.path().join("releases/1.0.0-123-456/kontor-mcp")).expect("read"),
        tampered.to_vec()
    );
    assert!(
        root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

#[test]
fn recovery_never_treats_an_absent_expected_pointer_as_clean_install() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal_with_previous(
        root_holder.path(),
        "promoted",
        &version.manifest,
        "123-456",
        serde_json::json!({"generation": "1.0.0-111-222", "release": "1.0.0"}),
    );
    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
    let root = root_holder.path().to_path_buf();
    let journal_before = journal_bytes(&root);
    let generation_before = directory_inventory(&root.join("releases/1.0.0-123-456"));
    let installer = installer(&root);
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        pointer_observation(&root),
        "absent",
        "an absent expected previous pointer is never clean-install admission; \
         the exact current name is absent by symlink_metadata NotFound"
    );
    assert_eq!(
        journal_bytes(&root),
        journal_before,
        "the exact journal bytes are retained"
    );
    assert_eq!(
        directory_inventory(&root.join("releases/1.0.0-123-456")),
        generation_before,
        "the generation's complete byte manifest is unchanged"
    );
}

#[test]
fn recovery_preserves_an_incomplete_staged_generation_with_changed_content() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "staged", &version.manifest, "123-456");
    let staging = write_owned_staging(root_holder.path(), &version.manifest, "123-456");
    std::fs::write(staging.join("kontor-mcp"), b"incomplete changed content").expect("tamper");
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        std::fs::read_to_string(staging.join("kontor-mcp")).expect("read"),
        "incomplete changed content"
    );
    assert_eq!(current_target(root_holder.path()), None);
    assert!(
        root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

#[test]
fn recovery_completes_a_complete_staged_generation() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "staged", &version.manifest, "123-456");
    write_complete_staging(root_holder.path(), "123-456", &version.manifest);
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::Completed)
    );
    assert_whole_release(root_holder.path(), "1.0.0");
    assert_eq!(
        current_target(root_holder.path()).as_deref(),
        Some("releases/1.0.0-123-456")
    );
    assert!(
        !root_holder
            .path()
            .join(INTERNAL)
            .join("staging-123-456")
            .exists()
    );
    assert!(
        !root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

#[test]
fn recovery_completes_from_incomplete_owned_staging_with_verified_promoted_generation() {
    // The missing compound variant: incomplete owned staging (a changed
    // artifact digest makes staging_is_complete false and blocks cleanup)
    // plus an independently verified already-promoted generation. Recovery
    // must inspect the promoted generation before the staging disposition and
    // complete, not roll back; the incomplete staging is preserved as
    // evidence and the journal is removed only after verify_active.
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "staged", &version.manifest, "123-456");
    let staging = write_owned_staging(root_holder.path(), &version.manifest, "123-456");
    std::fs::write(
        staging.join("kontor-mcp"),
        b"incomplete owned staging content",
    )
    .expect("tamper");
    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
    let inventory_before = directory_inventory(&staging);
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::Completed)
    );
    assert_whole_release(root_holder.path(), "1.0.0");
    assert_eq!(
        current_target(root_holder.path()).as_deref(),
        Some("releases/1.0.0-123-456")
    );
    assert!(
        !root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
    assert_eq!(
        directory_inventory(&staging),
        inventory_before,
        "the incomplete staging is preserved unchanged, not rolled back"
    );
    assert_eq!(
        file_bytes(&staging.join("kontor-mcp")),
        b"incomplete owned staging content"
    );
}

#[test]
fn recovery_completes_a_verified_promoted_journal() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "promoted", &version.manifest, "123-456");
    write_generation(root_holder.path(), "1.0.0-123-456", &version.manifest);
    let installer = installer(root_holder.path());
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::Completed)
    );
    assert_whole_release(root_holder.path(), "1.0.0");
    assert!(
        !root_holder
            .path()
            .join(INTERNAL)
            .join("journal.json")
            .exists()
    );
}

/// The three admissible pre-activation recovery branch shapes.
#[derive(Clone, Copy, Debug)]
enum RecoverySetup {
    /// Journal staged with a complete staging directory; recovery promotes.
    StagedPresent,
    /// Journal staged with the generation already promoted (staging missing).
    PromotedBeforeJournal,
    /// Journal promoted with the generation promoted.
    Promoted,
}

fn write_recovery_setup(root: &Path, manifest: &ArtifactManifest, setup: RecoverySetup) {
    match setup {
        RecoverySetup::StagedPresent => {
            write_journal(root, "staged", manifest, "123-456");
            write_complete_staging(root, "123-456", manifest);
        }
        RecoverySetup::PromotedBeforeJournal => {
            write_journal(root, "staged", manifest, "123-456");
            write_generation(root, "1.0.0-123-456", manifest);
        }
        RecoverySetup::Promoted => {
            write_journal(root, "promoted", manifest, "123-456");
            write_generation(root, "1.0.0-123-456", manifest);
        }
    }
}

/// The activation boundary a recovery tamper lands on.
#[derive(Clone, Copy, Debug)]
enum TamperBoundary {
    /// The last pre-effect boundary: the temporary exists, nothing activated.
    AfterActivationTemp,
    /// After the activation effect: the exchange happened, only the final
    /// active verification remains before journal removal.
    AfterSwap,
}

/// A recovery tamper at one actual activation boundary. Both content-only and
/// coherent marker+content tamper must end in PreservedConflict, never a
/// false completion, with the exact journal bytes retained and the complete
/// tampered bytes preserved. Before activation the pointer must be absent by
/// exact NotFound; after activation the pointer must name exactly the
/// activated generation.
fn recovery_activation_boundary_tamper(
    setup: RecoverySetup,
    boundary: TamperBoundary,
    coherent: bool,
) {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_recovery_setup(root_holder.path(), &version.manifest, setup);
    let root = root_holder.path().to_path_buf();
    let journal_before: std::rc::Rc<std::cell::RefCell<Option<Vec<u8>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let generation_cell: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let pointer_identity: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_journal = std::rc::Rc::clone(&journal_before);
    let hook_generation = std::rc::Rc::clone(&generation_cell);
    let hook_pointer = std::rc::Rc::clone(&pointer_identity);
    let hook_root = root.clone();
    let content: &[u8] = if coherent {
        b"coherent recovery boundary tamper"
    } else {
        b"content recovery boundary tamper"
    };
    let point = match boundary {
        TamperBoundary::AfterActivationTemp => FailPoint::AfterActivationTemp,
        TamperBoundary::AfterSwap => FailPoint::AfterSwap(ArtifactName::Mcp),
    };
    let mut hooks = Hooks(vec![hook(point, move |_| {
        let journal = std::fs::read(hook_root.join(INTERNAL).join("journal.json"))
            .map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_slice(&journal).map_err(|_| InstallError::Io)?;
        let generation = value["generation"]
            .as_str()
            .ok_or(InstallError::Io)?
            .to_owned();
        *hook_journal.borrow_mut() = Some(journal);
        *hook_generation.borrow_mut() = Some(generation.clone());
        // Capture the exact pointer identity when a pointer already exists
        // (the post-activation boundary); absence is expected before
        // activation and is not captured.
        if std::fs::symlink_metadata(hook_root.join("current")).is_ok() {
            *hook_pointer.borrow_mut() = Some(symlink_identity(&hook_root.join("current")));
        }
        if coherent {
            coherently_tamper_generation(&hook_root, content)
        } else {
            std::fs::write(
                hook_root
                    .join("releases")
                    .join(&generation)
                    .join("kontor-mcp"),
                content,
            )
            .map_err(|_| InstallError::Io)
        }
    })]);
    let installer = installer(&root);
    assert_eq!(
        installer.recover(&mut hooks),
        Ok(RecoveryOutcome::PreservedConflict),
        "{setup:?} {boundary:?} coherent={coherent} must preserve, not complete"
    );
    let generation = generation_cell.borrow().clone().expect("generation");
    assert_eq!(
        file_bytes(&root.join("releases").join(&generation).join("kontor-mcp")),
        content,
        "{setup:?}: the complete tampered bytes are preserved"
    );
    match boundary {
        TamperBoundary::AfterActivationTemp => {
            assert_eq!(
                pointer_observation(&root),
                "absent",
                "{setup:?}: no activation leaves the pointer absent by exact NotFound"
            );
        }
        TamperBoundary::AfterSwap => {
            let pointer = std::fs::symlink_metadata(root.join("current"))
                .expect("the activated pointer exists");
            assert!(pointer.file_type().is_symlink());
            assert_eq!(
                std::fs::read_link(root.join("current")).expect("pointer target"),
                PathBuf::from(format!("releases/{generation}")),
                "{setup:?}: the activated pointer names exactly the tampered generation"
            );
            assert_eq!(
                symlink_identity(&root.join("current")),
                pointer_identity
                    .borrow()
                    .clone()
                    .expect("captured pointer identity"),
                "{setup:?}: the activated pointer keeps its exact identity"
            );
        }
    }
    assert_eq!(
        journal_bytes(&root),
        journal_before.borrow().clone().expect("captured journal"),
        "{setup:?}: the exact journal bytes are retained"
    );
    if coherent {
        let marker: serde_json::Value = serde_json::from_slice(&file_bytes(
            &root
                .join("releases")
                .join(&generation)
                .join(".kontor-generation.json"),
        ))
        .expect("marker json");
        let digest = marker["artifacts"]
            .as_array()
            .expect("artifacts")
            .iter()
            .find(|artifact| artifact["name"] == "mcp")
            .expect("mcp entry")["sha256"]
            .as_str()
            .expect("digest")
            .to_owned();
        assert_eq!(
            digest,
            sha256_hex(content),
            "{setup:?}: the coherent marker binds the tampered bytes"
        );
    }
}

const EVERY_SETUP: [RecoverySetup; 3] = [
    RecoverySetup::StagedPresent,
    RecoverySetup::PromotedBeforeJournal,
    RecoverySetup::Promoted,
];

#[test]
fn recovery_after_activation_temp_content_tamper_preserves_every_branch() {
    for setup in EVERY_SETUP {
        recovery_activation_boundary_tamper(setup, TamperBoundary::AfterActivationTemp, false);
    }
}

#[test]
fn recovery_after_activation_temp_coherent_tamper_preserves_every_branch() {
    for setup in EVERY_SETUP {
        recovery_activation_boundary_tamper(setup, TamperBoundary::AfterActivationTemp, true);
    }
}

#[test]
fn recovery_after_swap_content_tamper_preserves_every_branch() {
    for setup in EVERY_SETUP {
        recovery_activation_boundary_tamper(setup, TamperBoundary::AfterSwap, false);
    }
}

#[test]
fn recovery_after_swap_coherent_tamper_preserves_every_branch() {
    for setup in EVERY_SETUP {
        recovery_activation_boundary_tamper(setup, TamperBoundary::AfterSwap, true);
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_generation_marker_is_preserved() {
    use std::os::unix::fs::symlink;
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    let staging = write_owned_staging(root_holder.path(), &version.manifest, "123-456");
    symlink(
        "elsewhere-marker.json",
        staging.join(".kontor-generation.json"),
    )
    .expect("marker link");
    let root = root_holder.path().to_path_buf();
    let inventory_before = directory_inventory(&staging);
    let journal_before = journal_bytes(&root);
    let installer = installer(&root);
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        std::fs::read_link(staging.join(".kontor-generation.json")).expect("readlink"),
        Path::new("elsewhere-marker.json")
    );
    assert_eq!(
        directory_inventory(&staging),
        inventory_before,
        "the complete staging inventory is unchanged"
    );
    assert_eq!(journal_bytes(&root), journal_before);
}

#[test]
fn an_oversized_generation_marker_is_preserved() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    let staging = write_owned_staging(root_holder.path(), &version.manifest, "123-456");
    std::fs::write(
        staging.join(".kontor-generation.json"),
        vec![b'{'; (MAX_DOCUMENT_BYTES + 1) as usize],
    )
    .expect("oversized marker");
    let root = root_holder.path().to_path_buf();
    let inventory_before = directory_inventory(&staging);
    let journal_before = journal_bytes(&root);
    let installer = installer(&root);
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        std::fs::metadata(staging.join(".kontor-generation.json"))
            .expect("metadata")
            .len(),
        MAX_DOCUMENT_BYTES + 1
    );
    assert_eq!(
        directory_inventory(&staging),
        inventory_before,
        "the complete staging inventory, including the oversized marker bytes, is unchanged"
    );
    assert_eq!(journal_bytes(&root), journal_before);
}

#[cfg(unix)]
#[test]
fn a_privilege_dependent_unreadable_generation_marker_is_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    let staging = write_owned_staging(root_holder.path(), &version.manifest, "123-456");
    let marker = staging.join(".kontor-generation.json");
    std::fs::write(&marker, b"{}").expect("marker");
    let root = root_holder.path().to_path_buf();
    let inventory_before = directory_inventory(&staging);
    let journal_before = journal_bytes(&root);
    let mut permissions = std::fs::metadata(&marker).expect("metadata").permissions();
    permissions.set_mode(0o000);
    std::fs::set_permissions(&marker, permissions).expect("chmod");
    let installer = installer(&root);
    // Privilege-dependent case, retained as such: a non-privileged reader gets
    // an observation error; a privileged reader reads malformed `{}`. These
    // are two distinct preservation reasons, and the deterministic
    // kind-observation fixtures (directory/FIFO) carry the read-error-as-
    // absence seed independently.
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    let mut restore = std::fs::metadata(&marker).expect("metadata").permissions();
    restore.set_mode(0o600);
    std::fs::set_permissions(&marker, restore).expect("restore");
    assert_eq!(
        directory_inventory(&staging),
        inventory_before,
        "the complete staging inventory is unchanged after mode restoration"
    );
    assert_eq!(journal_bytes(&root), journal_before);
}

#[cfg(unix)]
#[test]
fn a_fifo_generation_marker_is_preserved() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    let staging = write_owned_staging(root_holder.path(), &version.manifest, "123-456");
    let status = std::process::Command::new("mkfifo")
        .arg(staging.join(".kontor-generation.json"))
        .status()
        .expect("mkfifo");
    assert!(status.success());
    let root = root_holder.path().to_path_buf();
    let inventory_before = directory_inventory(&staging);
    let journal_before = journal_bytes(&root);
    let installer = installer(&root);
    // A deterministic kind observation error: reading a FIFO is refused as a
    // non-regular node without a blocking open.
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        directory_inventory(&staging),
        inventory_before,
        "the FIFO marker and the complete staging inventory are unchanged"
    );
    assert_eq!(journal_bytes(&root), journal_before);
}

#[test]
fn an_artifact_directory_in_staging_is_preserved() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    let staging = write_owned_staging(root_holder.path(), &version.manifest, "123-456");
    std::fs::remove_file(staging.join("kontor-mcp")).expect("remove");
    std::fs::create_dir(staging.join("kontor-mcp")).expect("artifact directory");
    std::fs::write(staging.join("kontor-mcp/nested"), b"nested").expect("nested");
    let root = root_holder.path().to_path_buf();
    let inventory_before = directory_inventory(&staging);
    let journal_before = journal_bytes(&root);
    let installer = installer(&root);
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert!(staging.join("kontor-mcp").is_dir());
    assert_eq!(
        std::fs::read_to_string(staging.join("kontor-mcp/nested")).expect("read"),
        "nested"
    );
    assert_eq!(
        directory_inventory(&staging),
        inventory_before,
        "the nested directory and the complete staging inventory are unchanged"
    );
    assert_eq!(journal_bytes(&root), journal_before);
}

/// Patch one generation marker's JSON in place, keeping every other field.
fn patch_marker(root: &Path, generation: &str, mutate: impl FnOnce(&mut serde_json::Value)) {
    let path = root
        .join("releases")
        .join(generation)
        .join(".kontor-generation.json");
    let mut marker: serde_json::Value =
        serde_json::from_slice(&file_bytes(&path)).expect("marker json");
    mutate(&mut marker);
    std::fs::write(&path, serde_json::to_vec(&marker).expect("marker bytes"))
        .expect("marker write");
}

#[test]
fn a_release_only_marker_mismatch_preserves_the_staging() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    write_complete_staging(root_holder.path(), "123-456", &version.manifest);
    let staging = root_holder.path().join(INTERNAL).join("staging-123-456");
    // Patch the staging marker itself (the staging directory carries its own).
    let path = staging.join(".kontor-generation.json");
    let mut marker: serde_json::Value =
        serde_json::from_slice(&file_bytes(&path)).expect("marker json");
    marker["release"] = serde_json::json!("9.9.9");
    std::fs::write(&path, serde_json::to_vec(&marker).expect("marker bytes")).expect("marker");
    let root = root_holder.path().to_path_buf();
    let inventory_before = directory_inventory(&staging);
    let journal_before = journal_bytes(&root);
    let installer = installer(&root);
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        directory_inventory(&staging),
        inventory_before,
        "a release-only marker mismatch preserves the complete staging bytes"
    );
    assert_eq!(journal_bytes(&root), journal_before);
}

#[test]
fn a_version_only_marker_mismatch_preserves_the_staging() {
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    write_journal(root_holder.path(), "prepared", &version.manifest, "123-456");
    write_complete_staging(root_holder.path(), "123-456", &version.manifest);
    let staging = root_holder.path().join(INTERNAL).join("staging-123-456");
    let path = staging.join(".kontor-generation.json");
    let mut marker: serde_json::Value =
        serde_json::from_slice(&file_bytes(&path)).expect("marker json");
    for artifact in marker["artifacts"].as_array_mut().expect("artifacts") {
        if artifact["name"] == "mcp" {
            artifact["version"] = serde_json::json!("9.9.9");
        }
    }
    std::fs::write(&path, serde_json::to_vec(&marker).expect("marker bytes")).expect("marker");
    let root = root_holder.path().to_path_buf();
    let inventory_before = directory_inventory(&staging);
    let journal_before = journal_bytes(&root);
    let installer = installer(&root);
    assert_eq!(
        installer.recover(&mut NoFaults),
        Ok(RecoveryOutcome::PreservedConflict)
    );
    assert_eq!(
        directory_inventory(&staging),
        inventory_before,
        "a version-only marker mismatch preserves the complete staging bytes"
    );
    assert_eq!(journal_bytes(&root), journal_before);
}

/// Write one coherent active generation for an arbitrary release token:
/// artifact bytes, marker schema, generation, generated nonce, release,
/// per-artifact versions and hashes all agree, so only a targeted predicate
/// can refuse. Used for otherwise-coherent unsafe-release fixtures and their
/// safe controls.
fn write_coherent_generation(root: &Path, release: &str, nonce: &str) {
    let generation = format!("{release}-{nonce}");
    let directory = root.join("releases").join(&generation);
    std::fs::create_dir_all(&directory).expect("generation");
    let mut artifacts = Vec::new();
    for name in ArtifactName::ALL {
        let bytes = format!("{}@{release}", name.file_name());
        std::fs::write(directory.join(name.file_name()), &bytes).expect("artifact");
        artifacts.push(serde_json::json!({
            "name": name,
            "version": release,
            "sha256": sha256_hex(bytes.as_bytes()),
        }));
    }
    std::fs::write(
        directory.join(".kontor-generation.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "generation": generation,
            "release": release,
            "nonce": nonce,
            "artifacts": artifacts,
        }))
        .expect("marker bytes"),
    )
    .expect("marker");
}

#[cfg(unix)]
#[test]
fn an_unsafe_active_marker_release_is_refused_before_previous_journal_persistence() {
    use std::os::unix::fs::symlink;
    let root_holder = tempfile::tempdir().expect("root");
    let unsafe_release = "UNSAFE..RELEASE";
    let generation = format!("{unsafe_release}-123-456");
    // Otherwise coherent: valid generated nonce, consistent release/nonce
    // construction, matching artifact versions and bytes, so only the
    // safe-release predicate can refuse this marker.
    write_coherent_generation(root_holder.path(), unsafe_release, "123-456");
    let root = root_holder.path().to_path_buf();
    symlink(format!("releases/{generation}"), root.join("current")).expect("pointer");
    let pointer_before = pointer_observation(&root);
    let generation_before = directory_inventory(&root.join("releases").join(&generation));
    let version = release("1.0.0");
    // The local value is named root_installer so the helper `installer(...)`
    // remains callable for the safe-release control below.
    let root_installer = installer(&root);
    assert_eq!(
        root_installer.status(&version.manifest),
        Err(InstallError::ActiveUnverified),
        "an unsafe marker release is never verified state"
    );
    assert_eq!(
        root_installer.active_generation(),
        Err(InstallError::ActiveUnverified)
    );
    let version_two = release("2.0.0");
    let error = root_installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut NoFaults,
        )
        .expect_err("an unsafe active marker blocks the update");
    assert_eq!(error, InstallError::ActiveUnverified);
    assert!(
        !root.join(INTERNAL).join("journal.json").exists(),
        "no previous release may be persisted from an incoherent marker"
    );
    assert_eq!(pointer_observation(&root), pointer_before);
    assert_eq!(
        directory_inventory(&root.join("releases").join(&generation)),
        generation_before
    );
    // Coherence control: the same fixture shape with a safe release token is
    // accepted as observed state, proving the refusal above comes from the
    // safe-release predicate and not from nonce/construction/version masking.
    let control_holder = tempfile::tempdir().expect("control root");
    write_coherent_generation(control_holder.path(), "SAFERELEASE", "123-456");
    symlink(
        "releases/SAFERELEASE-123-456",
        control_holder.path().join("current"),
    )
    .expect("control pointer");
    let control = installer(control_holder.path());
    let statuses = control
        .status(&version.manifest)
        .expect("the safe control is verifiable");
    assert!(
        statuses.iter().all(|status| status.sha256.is_some()),
        "the safe control exposes verified artifact state"
    );
}

#[cfg(unix)]
#[test]
fn a_nonce_incoherent_active_marker_is_refused() {
    use std::os::unix::fs::symlink;
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    let generation = "1.0.0-not-a-nonce";
    write_generation(root_holder.path(), generation, &version.manifest);
    patch_marker(root_holder.path(), generation, |marker| {
        marker["nonce"] = serde_json::json!("not-a-nonce");
        marker["generation"] = serde_json::json!("1.0.0-not-a-nonce");
    });
    let root = root_holder.path().to_path_buf();
    symlink(format!("releases/{generation}"), root.join("current")).expect("pointer");
    let installer = installer(&root);
    assert_eq!(
        installer.status(&version.manifest),
        Err(InstallError::ActiveUnverified),
        "a nonce that is not the generated shape is refused"
    );
    assert_eq!(
        installer.active_generation(),
        Err(InstallError::ActiveUnverified)
    );
}

#[cfg(unix)]
#[test]
fn an_incoherent_generation_construction_is_refused() {
    use std::os::unix::fs::symlink;
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    // The directory name and marker.generation agree, but the declared
    // release/nonce construction does not name that generation.
    let generation = "2.0.0-123-456";
    write_generation(root_holder.path(), generation, &version.manifest);
    patch_marker(root_holder.path(), generation, |marker| {
        marker["release"] = serde_json::json!("1.0.0");
        marker["nonce"] = serde_json::json!("123-456");
        marker["generation"] = serde_json::json!("2.0.0-123-456");
    });
    let root = root_holder.path().to_path_buf();
    symlink(format!("releases/{generation}"), root.join("current")).expect("pointer");
    let installer = installer(&root);
    assert_eq!(
        installer.status(&version.manifest),
        Err(InstallError::ActiveUnverified),
        "the release/nonce construction must name the generation"
    );
}

#[cfg(unix)]
#[test]
fn a_version_incoherent_active_marker_is_refused() {
    use std::os::unix::fs::symlink;
    let root_holder = tempfile::tempdir().expect("root");
    let version = release("1.0.0");
    let generation = "1.0.0-123-456";
    write_generation(root_holder.path(), generation, &version.manifest);
    patch_marker(root_holder.path(), generation, |marker| {
        for artifact in marker["artifacts"].as_array_mut().expect("artifacts") {
            if artifact["name"] == "mcp" {
                artifact["version"] = serde_json::json!("2.0.0");
            }
        }
    });
    let root = root_holder.path().to_path_buf();
    symlink(format!("releases/{generation}"), root.join("current")).expect("pointer");
    let installer = installer(&root);
    assert_eq!(
        installer.status(&version.manifest),
        Err(InstallError::ActiveUnverified),
        "an artifact version that disagrees with the release is refused"
    );
}

#[test]
fn a_foreign_activation_temporary_is_preserved_with_exact_bytes() {
    let root_holder = tempfile::tempdir().expect("root");
    let version_one = release("1.0.0");
    let version_two = release("2.0.0");
    let installer = installer(root_holder.path());
    install(&installer, &version_one).expect("baseline");
    let root = root_holder.path().to_path_buf();
    let pointer_before = current_target(&root);
    let captured: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let captured_inode: std::rc::Rc<std::cell::RefCell<Option<(u64, u64)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_name = std::rc::Rc::clone(&captured);
    let hook_inode = std::rc::Rc::clone(&captured_inode);
    let hook_root = root.clone();
    let mut hooks = Hooks(vec![hook(FailPoint::BeforeActivate, move |_| {
        let internal = hook_root.join(INTERNAL);
        let journal =
            std::fs::read_to_string(internal.join("journal.json")).map_err(|_| InstallError::Io)?;
        let value: serde_json::Value =
            serde_json::from_str(&journal).map_err(|_| InstallError::Io)?;
        let nonce = value["nonce"].as_str().ok_or(InstallError::Io)?;
        let name = format!(".current-{nonce}.tmp");
        std::fs::write(hook_root.join(&name), b"foreign activation temporary")
            .map_err(|_| InstallError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata =
                std::fs::metadata(hook_root.join(&name)).map_err(|_| InstallError::Io)?;
            *hook_inode.borrow_mut() = Some((metadata.dev(), metadata.ino()));
        }
        *hook_name.borrow_mut() = Some(name);
        Ok(())
    })]);
    let error = installer
        .install(
            &version_two.manifest,
            &version_two.sources,
            &version_two.dir(),
            &mut hooks,
        )
        .expect_err("a foreign activation temporary is never reclaimed");
    assert_eq!(error, InstallError::ActivationChanged);
    assert_eq!(current_target(&root), pointer_before);
    let preserved = root.join(captured.borrow().as_deref().expect("captured name"));
    assert_eq!(
        std::fs::read_to_string(&preserved).expect("read"),
        "foreign activation temporary"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(&preserved).expect("metadata");
        assert_eq!(
            (metadata.dev(), metadata.ino()),
            captured_inode.borrow().expect("captured inode")
        );
    }
    assert!(root.join(INTERNAL).join("journal.json").exists());
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[test]
fn unsupported_targets_refuse_status_recovery_and_directory_operations() {
    use kontor_bootstrap::confinement::ConfinementError;
    let root_holder = tempfile::tempdir().expect("root");
    let installer = installer(root_holder.path());
    let artifacts: Vec<ArtifactEntry> = ArtifactName::ALL
        .into_iter()
        .map(|name| ArtifactEntry {
            name,
            version: "1.0.0".to_owned(),
            sha256: "a".repeat(64),
        })
        .collect();
    let manifest = ArtifactManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        release: "1.0.0".to_owned(),
        artifacts,
    };
    assert_eq!(
        installer.status(&manifest),
        Err(InstallError::UnsupportedPlatform)
    );
    assert_eq!(
        installer.active_generation(),
        Err(InstallError::UnsupportedPlatform)
    );
    assert_eq!(
        installer.recover(&mut NoFaults),
        Err(InstallError::UnsupportedPlatform)
    );
    assert_eq!(
        Dir::open_root(root_holder.path()).unwrap_err(),
        ConfinementError::UnsupportedPlatform
    );
    // Public static fd operations on an independently opened file are fenced
    // before any effect or observation: an unsupported target may not change
    // permissions or read an identity through them.
    let external = root_holder.path().join("external-file");
    std::fs::write(&external, b"external").expect("external file");
    // Set and assert an initial mode explicitly different from the requested
    // 0o640, so a refusal-after-fchmod relocation cannot pass by matching an
    // accidental initial mode.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut initial = std::fs::metadata(&external)
            .expect("metadata")
            .permissions();
        initial.set_mode(0o604);
        std::fs::set_permissions(&external, initial).expect("initial mode");
    }
    let before_bytes = file_bytes(&external);
    #[cfg(unix)]
    let before_mode = {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&external)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o604,
            "the initial mode precondition differs from the requested 0o640"
        );
        mode
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&external)
        .expect("open external");
    assert_eq!(
        Dir::file_identity(&file).unwrap_err(),
        ConfinementError::UnsupportedPlatform
    );
    assert_eq!(
        Dir::set_file_mode(&file, 0o640).unwrap_err(),
        ConfinementError::UnsupportedPlatform
    );
    #[cfg(unix)]
    assert_eq!(
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(&external)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777
        },
        before_mode,
        "the refused permission operation changes no mode"
    );
    assert_eq!(
        file_bytes(&external),
        before_bytes,
        "the refused fd operations change no bytes"
    );
    assert_eq!(
        std::fs::read_dir(root_holder.path()).expect("root").count(),
        1,
        "only the test-created external file exists and no effect ran"
    );
}
