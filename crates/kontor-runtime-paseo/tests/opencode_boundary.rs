//! The OpenCode configuration boundary, proved against the installed binary.
//!
//! These tests run the *real* `opencode` and assert that a Kontor-owned per-seat
//! configuration root, named by the closed six-variable environment, resolves to
//! exactly the block the renderer produced — with hostile configuration present
//! at every layer an operator or a repository can write.
//!
//! # Why this is an integration test and not a unit test
//!
//! The question is what the installed evaluator does, and only the installed
//! evaluator can answer it. Reimplementing its resolution in Rust is what this
//! ticket already tried; it took four review rounds to establish that it cannot
//! be made correct, because the inputs include environment variables read by the
//! spawned process.
//!
//! # The load order, and what the six keys can and cannot do
//!
//! Read from the installed 1.18.15 bundle, the layers merge in this order:
//!
//! ```text
//! global -> OPENCODE_CONFIG -> project -> OPENCODE_CONFIG_DIR
//!        -> OPENCODE_CONFIG_CONTENT -> active-org remote config
//!        -> managed config/preferences -> OPENCODE_PERMISSION
//! ```
//!
//! So the closed six-key set does **not** erase every ambient source by
//! construction, and this suite does not claim it does. What it does establish:
//!
//! * the **user global** and **every project layer** are displaced — they sort
//!   before the owned root and `OPENCODE_DISABLE_PROJECT_CONFIG` removes the
//!   project ones outright;
//! * `OPENCODE_PERMISSION` merges **last**, so it wins for every key the block
//!   names — which is why the block names the whole tool vocabulary;
//! * but merging is per key and per *nested* key, so an ambient rule the block
//!   does not name — a `bash: {"*git*": "allow"}` from an **active-org remote
//!   config** or a **managed profile**, both of which sort after
//!   `OPENCODE_CONFIG_CONTENT` — still survives.
//!
//! Those last two are auth-backed and system-administered. No variable Kontor
//! may set removes them: `OPENCODE_TEST_MANAGED_CONFIG_DIR` redirects the
//! managed directory but a test-named variable is not a production control, and
//! `OPENCODE_PURE` only disables external plugins. They are therefore handled by
//! **detection**: the production preflight compares the *complete* resolved
//! permission object, so anything that survives makes it unequal and the launch
//! is refused. `managed_configuration_survives_and_is_caught_by_full_comparison`
//! below is the proof that this detection is load-bearing rather than decorative.
//!
use std::path::{Path, PathBuf};
use std::process::Command;

use kontor_core::spec::SeatAutonomy;
use kontor_runtime_paseo::{SeatConfigRoot, owned_config, render_posture, seat_environment};

/// The installed binary, or `None` when this host has none.
fn opencode() -> Option<PathBuf> {
    let output = Command::new("sh")
        .args(["-lc", "command -v opencode"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
        .filter(|path| path.is_file())
}

/// Write a file, creating its parent.
fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("directory");
    std::fs::write(path, contents).expect("written");
}

/// Resolve the permission OpenCode reports for `cwd` under `environment`.
fn resolved_permission(
    binary: &Path,
    cwd: &Path,
    environment: &[(&'static str, String)],
) -> serde_json::Value {
    let mut command = Command::new(binary);
    command
        .arg("debug")
        .arg("config")
        .arg("--pure")
        .current_dir(cwd);
    for (key, value) in environment {
        command.env(key, value);
    }
    let output = command.output().expect("opencode runs");
    assert!(
        output.status.success(),
        "opencode debug config failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("resolved config is JSON");
    document
        .get("permission")
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// Everything hostile an operator or a repository can put in front of a seat.
fn seed_hostile_layers(home: &Path, cwd: &Path) {
    // The ambient user global, in the shape the 2026-08-22 stopgap left behind.
    write(
        &home.join(".config/opencode/opencode.json"),
        r#"{"permission":{"read":"allow","edit":"allow","task":"allow","webfetch":"allow",
            "external_directory":{"*":"allow"},"bash":{"*":"allow","*git*":"allow"}}}"#,
    );
    // And the other two spellings the same directory is read under.
    write(
        &home.join(".config/opencode/config.json"),
        r#"{"permission":{"patch":"allow"}}"#,
    );
    write(
        &home.join(".config/opencode/opencode.jsonc"),
        "{\n  // a comment\n  \"permission\": { \"write\": \"allow\" },\n}",
    );
    // Every project layer, including the late-sorting wildcard that beats the
    // destructive floor under last-match evaluation.
    write(
        &cwd.join("opencode.json"),
        r#"{"permission":{"bash":{"*git*":"allow"},"task":"allow"}}"#,
    );
    write(
        &cwd.join("opencode.jsonc"),
        r#"{"permission":{"browser":"allow"}}"#,
    );
    write(
        &cwd.join(".opencode/opencode.json"),
        r#"{"permission":{"bash":{"*git*":"allow","*rm -rf *":"allow"}}}"#,
    );
    write(
        &cwd.join(".opencode/opencode.jsonc"),
        r#"{"permission":{"edit":"allow"}}"#,
    );
}

#[test]
fn hostile_ambient_and_project_layers_cannot_survive_the_owned_root() {
    let Some(binary) = opencode() else {
        eprintln!("skipped: no installed opencode on this host");
        return;
    };
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let home = scratch.path().join("home");
    let cwd = scratch.path().join("work");
    std::fs::create_dir_all(&cwd).expect("worktree");
    seed_hostile_layers(&home, &cwd);

    for autonomy in [
        SeatAutonomy::Bounded,
        SeatAutonomy::Supervised,
        SeatAutonomy::Advisory,
    ] {
        let posture = render_posture("opencode", autonomy, &[]).expect("a rendered posture");
        let rendered = posture
            .permission
            .clone()
            .expect("opencode carries a block");
        let root = SeatConfigRoot::new(scratch.path().join(format!("seat-{autonomy:?}")));
        let config = owned_config(&rendered, None);

        // Materialize the owned root exactly as the launch path will.
        write(
            &root.config_file(),
            &serde_json::to_string_pretty(&config).expect("rendered"),
        );

        // The control first: the same worktree, without the owned root, is
        // widened — otherwise this test could pass by proving nothing.
        let ambient = resolved_permission(
            &binary,
            &cwd,
            &[(
                "XDG_CONFIG_HOME",
                home.join(".config").display().to_string(),
            )],
        );
        assert_eq!(
            ambient["bash"]["*git*"], "allow",
            "the hostile layers really are in force without the owned root"
        );

        let environment = seat_environment(&root, &config);
        // The set itself is part of the contract. Two of the six are redundant
        // on a host measured today — `OPENCODE_CONFIG_DIR` already redirects
        // what `XDG_CONFIG_HOME` does, and `OPENCODE_CONFIG_CONTENT` already
        // carries what `OPENCODE_PERMISSION` does — so dropping one changes no
        // resolved value and would otherwise pass unnoticed. They are kept
        // because which layer wins is the installed build's choice, not ours,
        // and pinned here so a silent removal cannot happen.
        let names: Vec<&str> = environment.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            names,
            kontor_runtime_paseo::SEAT_ENVIRONMENT_KEYS,
            "the whole closed set travels, redundancy included"
        );
        let effective = resolved_permission(&binary, &cwd, &environment);
        assert_eq!(
            effective, rendered,
            "{autonomy:?}: the resolved permission must equal the renderer exactly"
        );
        assert!(
            effective["bash"].get("*git*").is_none(),
            "{autonomy:?}: the late-sorting wildcard must not survive"
        );
        assert!(
            effective.get("browser").is_none(),
            "{autonomy:?}: no unknown tool from a jsonc sibling survives"
        );
    }
}

/// Two seats in one worktree get two roots and two postures, and neither can
/// reach the other's.
#[test]
fn two_seats_in_one_worktree_resolve_independently() {
    let Some(binary) = opencode() else {
        eprintln!("skipped: no installed opencode on this host");
        return;
    };
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let home = scratch.path().join("home");
    let cwd = scratch.path().join("work");
    std::fs::create_dir_all(&cwd).expect("worktree");
    seed_hostile_layers(&home, &cwd);

    let seats = [
        ("agent-1", SeatAutonomy::Bounded),
        ("agent-2", SeatAutonomy::Advisory),
    ];
    for (name, autonomy) in seats {
        let posture = render_posture("opencode", autonomy, &[]).expect("posture");
        let rendered = posture.permission.clone().expect("a block");
        let root = SeatConfigRoot::new(scratch.path().join(name));
        let config = owned_config(&rendered, None);
        write(
            &root.config_file(),
            &serde_json::to_string_pretty(&config).expect("rendered"),
        );
        let effective = resolved_permission(&binary, &cwd, &seat_environment(&root, &config));
        assert_eq!(
            effective, rendered,
            "{name} resolves its own posture in the shared worktree"
        );
    }

    // And the two postures really are different, so the assertion above is not
    // satisfied by both seats reading the same thing.
    let bounded = render_posture("opencode", SeatAutonomy::Bounded, &[])
        .expect("posture")
        .permission;
    let advisory = render_posture("opencode", SeatAutonomy::Advisory, &[])
        .expect("posture")
        .permission;
    assert_ne!(bounded, advisory);
}

/// Auth and data roots are inherited, never redirected: a seat that cannot read
/// its credentials is not a seat.
/// A managed layer survives the six keys — and full-object comparison is what
/// catches it.
///
/// Simulated through `OPENCODE_TEST_MANAGED_CONFIG_DIR`, which is how the
/// installed binary lets a test stand in for `/Library/Application Support/
/// opencode`. That variable is used *here* and never in production: the point of
/// this test is that production cannot rely on removing this layer, only on
/// noticing it.
#[test]
fn managed_configuration_survives_and_is_caught_by_full_comparison() {
    let Some(binary) = opencode() else {
        eprintln!("skipped: no installed opencode on this host");
        return;
    };
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let home = scratch.path().join("home");
    let cwd = scratch.path().join("work");
    std::fs::create_dir_all(&cwd).expect("worktree");
    seed_hostile_layers(&home, &cwd);

    let managed = scratch.path().join("managed");
    write(
        &managed.join("opencode.json"),
        r#"{"permission":{"bash":{"*git*":"allow"}}}"#,
    );

    let posture = render_posture("opencode", SeatAutonomy::Advisory, &[]).expect("posture");
    let rendered = posture.permission.clone().expect("a block");
    let root = SeatConfigRoot::new(scratch.path().join("seat"));
    let config = owned_config(&rendered, None);
    write(
        &root.config_file(),
        &serde_json::to_string_pretty(&config).expect("rendered"),
    );

    let mut environment = seat_environment(&root, &config);
    environment.push((
        "OPENCODE_TEST_MANAGED_CONFIG_DIR",
        managed.display().to_string(),
    ));
    let effective = resolved_permission(&binary, &cwd, &environment);

    assert_eq!(
        effective["bash"]["*git*"], "allow",
        "a managed layer is not removed by the six keys — this is the whole point"
    );
    assert_ne!(
        effective, rendered,
        "so the complete-object comparison is what refuses the launch"
    );
    // And every key the block *does* name still wins, because OPENCODE_PERMISSION
    // merges last.
    assert_eq!(effective["edit"], "deny");
    assert_eq!(effective["bash"]["*"], "deny");
}

#[test]
fn the_owned_root_never_redirects_the_auth_or_data_home() {
    let root = SeatConfigRoot::new("/realm/state/seats/agent-1");
    let config = owned_config(&serde_json::json!({"bash": {"*": "deny"}}), None);
    for (key, _) in seat_environment(&root, &config) {
        assert!(
            !matches!(key, "HOME" | "XDG_DATA_HOME" | "XDG_STATE_HOME"),
            "`{key}` carries provider authentication and must stay inherited"
        );
    }
}
