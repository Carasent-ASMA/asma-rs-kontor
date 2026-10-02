//! Artifact-layer security regressions: strict journal validation, generation
//! activation without mixed active sets, confined promotion, and recovery that
//! never removes or overwrites foreign bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kontor_bootstrap::artifact::manifest::{
    ArtifactEntry, ArtifactManifest, ArtifactName, MANIFEST_SCHEMA_VERSION, VerifiedSource,
    sha256_hex,
};
use kontor_bootstrap::confinement::Dir;
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
                | FailPoint::BeforeFinalize
                | FailPoint::AfterFinalize
        );
        let expected_outcome = if matches!(
            point,
            FailPoint::BeforeSwap(_) | FailPoint::AfterSwap(_) | FailPoint::BeforeFinalize
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
        assert_eq!(error, InstallError::ActiveUnverified);
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
