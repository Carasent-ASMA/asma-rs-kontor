//! KON-MVP-18 — the opt-in live-harness probe.
//!
//! Everything the pilot proves, it proves deterministically against scripted
//! runtimes. This target answers a different and much narrower question: *are
//! the harnesses this machine has installed the ones the pilot stands in for,
//! and what versions are they?*
//!
//! It is opt-in because a test that silently passes when a runtime is absent
//! teaches an operator to read green as "the live harness works". Without
//! `KONTOR_PILOT_LIVE=1` it reports that it is disabled and asserts nothing.
//!
//! It deliberately writes **only** an inventory, and only to the ephemeral root.
//! It answers no acceptance criterion, so it must not produce a verdict: a
//! second bundle whose forty-odd criteria all read `missing` would be noise an
//! inspector has to learn to ignore, and the whole point of the ledger is that
//! nothing in it can be ignored.
//!
//! ```sh
//! KONTOR_PILOT_LIVE=1 cargo test -p kontor-tests-e2e --test pilot_live -- --nocapture
//! ```

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use kontor_tests_e2e::{head_commit, repo_root};
use serde_json::{Value, json};

/// The environment variable that arms this probe.
const ARM: &str = "KONTOR_PILOT_LIVE";

/// One harness the pilot stands in for.
struct Harness {
    /// What the pilot calls it.
    plane: &'static str,
    /// The environment variable naming its executable, if the operator set one.
    executable_var: &'static str,
    /// The executable to look for on `PATH` when nothing names one.
    default_executable: &'static str,
    /// The argument that makes it print its version and exit.
    version_argument: &'static str,
}

/// The two runtimes the deterministic driver models, plus the ASMA connector.
const HARNESSES: &[Harness] = &[
    Harness {
        plane: "paseo",
        executable_var: "KONTOR_PASEO_EXECUTABLE",
        default_executable: "paseo",
        version_argument: "--version",
    },
    Harness {
        plane: "ao",
        executable_var: "KONTOR_AO_EXECUTABLE",
        default_executable: "ao",
        version_argument: "--version",
    },
    Harness {
        plane: "asma",
        executable_var: "KONTOR_ASMA_EXECUTABLE",
        default_executable: "asma",
        version_argument: "--version",
    },
];

#[test]
fn pilot_live() {
    if !std::env::var(ARM).is_ok_and(|value| value == "1") {
        println!(
            "KON-MVP-18 live probe: disabled. Set {ARM}=1 to verify the installed harnesses; \
             this run asserted nothing about them."
        );
        return;
    }

    let root = repo_root();
    let commit = head_commit(&root);
    let inventory: Vec<Value> = HARNESSES.iter().map(probe).collect();
    let missing: Vec<&str> = HARNESSES
        .iter()
        .zip(&inventory)
        .filter(|(_, entry)| entry.get("present").and_then(Value::as_bool) != Some(true))
        .map(|(harness, _)| harness.plane)
        .collect();

    let directory = root.join("target/kontor-pilot/live");
    fs::create_dir_all(&directory).expect("the live inventory directory is creatable");
    let document = json!({
        "schema_version": 1,
        "ticket": kontor_tests_e2e::TICKET,
        "commit": commit,
        "environment": "live-harness-probe",
        "answers_acceptance_criteria": false,
        "harnesses": inventory,
    });
    fs::write(
        directory.join("live-inventory.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&document).expect("the inventory serializes")
        ),
    )
    .expect("the live inventory is writable");

    println!(
        "KON-MVP-18 live probe: {} of {} harnesses present; inventory at {}",
        HARNESSES.len() - missing.len(),
        HARNESSES.len(),
        directory.join("live-inventory.json").display()
    );

    assert!(
        missing.is_empty(),
        "{ARM}=1 asks for the installed harnesses to be verified, but these are absent or did \
         not report a version: {missing:?}"
    );
}

/// Resolve one harness and read back whatever version it reports.
///
/// The recorded `program` is the file name only: an operator who points this at
/// a home-directory build should not have that path copied into an artifact.
fn probe(harness: &Harness) -> Value {
    let configured = std::env::var(harness.executable_var).ok();
    let program = configured
        .clone()
        .unwrap_or_else(|| harness.default_executable.to_owned());
    match Command::new(&program)
        .arg(harness.version_argument)
        .output()
    {
        Ok(output) if output.status.success() => json!({
            "plane": harness.plane,
            "present": true,
            "program": PathBuf::from(&program)
                .file_name()
                .map_or_else(|| program.clone(), |name| name.to_string_lossy().into_owned()),
            "configured_by": configured.map(|_| harness.executable_var),
            "version": String::from_utf8_lossy(&output.stdout).trim(),
        }),
        Ok(output) => json!({
            "plane": harness.plane,
            "present": false,
            "reason": "the executable answered a version request with a failure",
            "status": output.status.code(),
        }),
        Err(_) => json!({
            "plane": harness.plane,
            "present": false,
            "reason": "the executable was not resolvable",
        }),
    }
}
