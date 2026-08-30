//! Proving a seat's permission posture before anything native is created.
//!
//! # Why this runs a process
//!
//! What an OpenCode seat may do is decided by its *resolved* configuration, and
//! only the installed binary resolves it. This ticket already tried the
//! alternative — reimplementing the merge in Rust — and it took four review
//! rounds to establish that it cannot be made correct: the inputs include
//! environment variables read by the spawned process, an auth-backed active-org
//! remote configuration, and a system managed layer.
//!
//! So the posture is proved the only way it can be: run the same binary Paseo
//! will run, in the same working directory, with exactly the same closed
//! environment, and require the complete resolved permission object to equal the
//! one the renderer produced.
//!
//! # What equality is claimed over
//!
//! Narrowly, and deliberately not "the environment is equal":
//!
//! * **binary identity** — the absolute path and version Paseo's own
//!   `provider diagnostic` reports, not whatever this daemon's `PATH` finds;
//! * **working directory** — the seat's canonical worktree;
//! * **the closed six-key environment** — byte-identical to what `agent run`
//!   will carry;
//! * **the owned files** — hashed after readback.
//!
//! `HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME` are *inherited rather than
//! asserted equal*, because provider credentials live under them and a seat that
//! cannot authenticate is not a seat. That they still resolve where the daemon
//! says they do is checked separately, by comparing the binary's own reported
//! data root against the auth path in the diagnostic.
//!
//! Every failure — a non-zero exit, a timeout, unparseable output, a version
//! that is not the one Paseo resolves, or any difference in the permission
//! object — refuses the launch, and refuses it before any native call.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kontor_runtime::adapter::{RuntimeError, RuntimeResult};

/// How long the preflight waits for the binary to answer.
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);

/// The provider executable Paseo itself resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderBinary {
    /// The absolute path the daemon reports.
    pub path: PathBuf,
    /// The version the daemon reports it as.
    pub version: String,
    /// Where the daemon says the provider's credentials live.
    pub auth: Option<PathBuf>,
}

/// Read the daemon's own provider diagnostic.
///
/// The response is `{provider, diagnostic}` where `diagnostic` is a raw
/// multi-line report, so this reads exactly the three lines it needs and refuses
/// anything it cannot read unambiguously. It deliberately does not invent a
/// structured schema for a string.
///
/// # Errors
/// [`RuntimeError::LaunchNotAdmitted`] when the resolved path or version is
/// absent, empty, relative or stated more than once.
pub fn parse_provider_diagnostic(document: &serde_json::Value) -> RuntimeResult<ProviderBinary> {
    let refuse = |rule: &'static str| RuntimeError::LaunchNotAdmitted { rule };
    let report = document
        .get("diagnostic")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| refuse("the provider diagnostic carries no report"))?;

    let single = |field: &str, rule: &'static str| -> RuntimeResult<String> {
        let needle = format!("{field}:");
        let mut found: Option<String> = None;
        for line in report.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix(&needle) {
                let value = value.trim().to_owned();
                if value.is_empty() || found.is_some() {
                    return Err(refuse(rule));
                }
                found = Some(value);
            }
        }
        found.ok_or_else(|| refuse(rule))
    };

    let path = PathBuf::from(single(
        "Resolved path",
        "the provider diagnostic names no binary",
    )?);
    if !path.is_absolute() {
        return Err(refuse("the provider diagnostic names a relative binary"));
    }
    let version = single("Version", "the provider diagnostic names no version")?;
    // The auth line is evidence, not a requirement: a provider with no
    // credentials configured still reports a binary.
    let auth = report
        .lines()
        .find_map(|line| line.split_once("Credentials").map(|(_, rest)| rest))
        .map(|rest| PathBuf::from(strip_ansi(rest).trim()))
        .filter(|path| !path.as_os_str().is_empty());
    Ok(ProviderBinary {
        path,
        version,
        auth,
    })
}

/// Drop ANSI escape sequences from a diagnostic line.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            for escape in chars.by_ref() {
                if escape.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(character);
    }
    out
}

/// Run `<binary> debug config --pure` and return the resolved configuration.
///
/// `--pure` disables external plugins only; it is not a containment mechanism
/// and nothing here treats it as one. It is passed so a plugin cannot make the
/// preflight answer differ from a plain resolution.
fn resolve_configuration(
    binary: &Path,
    cwd: &Path,
    environment: &[(&'static str, String)],
    timeout: Duration,
) -> RuntimeResult<serde_json::Value> {
    let refuse = |rule: &'static str| RuntimeError::LaunchNotAdmitted { rule };
    let mut command = Command::new(binary);
    command
        .args(["debug", "config", "--pure"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Exactly the closed set, and nothing removed: HOME and the data and state
    // roots stay inherited so the seat's credentials resolve as they will at
    // spawn.
    for (key, value) in environment {
        command.env(key, value);
    }

    let mut child = command
        .spawn()
        .map_err(|_| refuse("the provider binary could not be run for the preflight"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(refuse("the preflight did not answer within its timeout"));
            }
            Err(_) => return Err(refuse("the preflight could not be waited on")),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|_| refuse("the preflight produced no readable output"))?;
    if !output.status.success() {
        return Err(refuse("the provider refused to resolve its configuration"));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| refuse("the resolved configuration was not readable JSON"))
}

/// Prove the seat's posture, or refuse the launch.
///
/// # Errors
/// [`RuntimeError::LaunchNotAdmitted`] on a version that is not the one Paseo
/// resolves, an unrunnable or slow binary, unreadable output, or any difference
/// between the resolved permission object and `expected`.
pub fn prove_posture(
    provider: &ProviderBinary,
    supported_versions: &[&str],
    cwd: &Path,
    expected: &serde_json::Value,
    environment: &[(&'static str, String)],
) -> RuntimeResult<()> {
    prove_posture_within(
        provider,
        supported_versions,
        cwd,
        expected,
        environment,
        PREFLIGHT_TIMEOUT,
    )
}

/// [`prove_posture`] with an explicit deadline.
///
/// # Errors
/// As [`prove_posture`].
pub fn prove_posture_within(
    provider: &ProviderBinary,
    supported_versions: &[&str],
    cwd: &Path,
    expected: &serde_json::Value,
    environment: &[(&'static str, String)],
    timeout: Duration,
) -> RuntimeResult<()> {
    let refuse = |rule: &'static str| RuntimeError::LaunchNotAdmitted { rule };
    if !supported_versions.contains(&provider.version.as_str()) {
        return Err(refuse(
            "the provider version Paseo resolves is not one this posture was proved against",
        ));
    }
    let resolved = resolve_configuration(&provider.path, cwd, environment, timeout)?;
    let permission = resolved
        .get("permission")
        .ok_or_else(|| refuse("the resolved configuration states no permission"))?;
    // The *whole* object. Comparing selected keys is what lets an ambient rule
    // the block never named — from a managed profile or an active-org remote
    // config, both of which merge after the owned content — survive unnoticed.
    if permission != expected {
        return Err(refuse(
            "the resolved permission does not equal the posture this seat was rendered for",
        ));
    }
    Ok(())
}

/// Check that the preserved roots still resolve where the daemon says they do.
///
/// Not a claim that the whole environment is equal — it is a bridge for the one
/// part deliberately left inherited. If the binary's own reported data root does
/// not contain the credentials the daemon's diagnostic names, then this process
/// and the spawned one would authenticate differently, and the preflight is
/// answering about a different seat.
///
/// # Errors
/// [`RuntimeError::LaunchNotAdmitted`] when the roots disagree.
pub fn prove_preserved_roots(
    provider: &ProviderBinary,
    reported_data_root: &Path,
) -> RuntimeResult<()> {
    let Some(auth) = provider.auth.as_ref() else {
        return Ok(());
    };
    let expanded = auth.to_string_lossy().replace('~', "");
    if expanded.is_empty() {
        return Ok(());
    }
    let matches = if reported_data_root.to_string_lossy().contains("opencode") {
        expanded
            .trim_start_matches('/')
            .starts_with(".local/share/opencode")
            || auth.starts_with(reported_data_root)
    } else {
        false
    };
    if !matches {
        return Err(RuntimeError::LaunchNotAdmitted {
            rule: "the preflight's data root is not the one the daemon reports credentials under",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact report the installed 0.6.1 daemon returns for OpenCode.
    const REAL_DIAGNOSTIC: &str = "OpenCode\n  Command source: default\n  Configured command: opencode\n  Daemon PATH: /Users/igor/.local/bin:/opt/homebrew/bin\n  Daemon shell: /bin/zsh\n  PATH matches: /opt/homebrew/bin/opencode\n  which -a opencode: /opt/homebrew/bin/opencode\n  Binary: opencode\n  Resolved path: /opt/homebrew/bin/opencode\n  Version: 1.18.15\n  Auth: \n    \u{250c}  Credentials \u{1b}[90m~/.local/share/opencode/auth.json\n  Models: 363\n  Status: Ready";

    fn diagnostic(report: &str) -> serde_json::Value {
        serde_json::json!({ "provider": "opencode", "diagnostic": report })
    }

    /// A stand-in binary that answers with `stdout` and exits `code`.
    fn stub(directory: &Path, stdout: &str, code: i32, sleep_seconds: u32) -> PathBuf {
        let path = directory.join("stub-opencode");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nsleep {sleep_seconds}\ncat <<'JSON'\n{stdout}\nJSON\nexit {code}\n"
            ),
        )
        .expect("written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("mode");
        }
        path
    }

    fn provider_at(path: PathBuf) -> ProviderBinary {
        ProviderBinary {
            path,
            version: "1.18.15".to_owned(),
            auth: None,
        }
    }

    #[test]
    fn the_real_diagnostic_yields_the_binary_paseo_resolves() {
        let parsed = parse_provider_diagnostic(&diagnostic(REAL_DIAGNOSTIC)).expect("parsed");
        assert_eq!(parsed.path, PathBuf::from("/opt/homebrew/bin/opencode"));
        assert_eq!(parsed.version, "1.18.15");
        assert_eq!(
            parsed.auth,
            Some(PathBuf::from("~/.local/share/opencode/auth.json")),
            "the credential path is read with its colour codes stripped"
        );
    }

    #[test]
    fn an_unreadable_diagnostic_is_refused_rather_than_guessed() {
        for report in [
            "OpenCode\n  Version: 1.18.15", // no resolved path
            "OpenCode\n  Resolved path: /opt/homebrew/bin/opencode", // no version
            "OpenCode\n  Resolved path: opencode\n  Version: 1", // relative
            "OpenCode\n  Resolved path: \n  Version: 1.18.15", // empty
            "OpenCode\n  Resolved path: /a\n  Resolved path: /b\n  Version: 1.18.15", // ambiguous
        ] {
            assert!(
                parse_provider_diagnostic(&diagnostic(report)).is_err(),
                "must refuse: {report}"
            );
        }
        assert!(parse_provider_diagnostic(&serde_json::json!({})).is_err());
    }

    #[test]
    fn a_matching_resolution_proves_the_posture() {
        let scratch = tempfile::TempDir::new().expect("scratch");
        let expected = serde_json::json!({"bash": {"*": "deny"}});
        let binary = stub(
            scratch.path(),
            &serde_json::json!({ "permission": expected }).to_string(),
            0,
            0,
        );
        prove_posture(
            &provider_at(binary),
            &["1.18.15"],
            scratch.path(),
            &expected,
            &[],
        )
        .expect("the resolved permission equals the posture");
    }

    #[test]
    fn every_way_the_proof_can_fail_refuses_the_launch() {
        let scratch = tempfile::TempDir::new().expect("scratch");
        let expected = serde_json::json!({"bash": {"*": "deny"}});
        let prove = |binary: PathBuf, versions: &[&str]| {
            prove_posture_within(
                &provider_at(binary),
                versions,
                scratch.path(),
                &expected,
                &[],
                Duration::from_millis(400),
            )
        };

        // one extra ambient rule the block never named
        let widened = stub(
            scratch.path(),
            &serde_json::json!({"permission": {"bash": {"*": "deny", "*git*": "allow"}}})
                .to_string(),
            0,
            0,
        );
        assert!(prove(widened, &["1.18.15"]).is_err(), "an extra rule");

        // a missing rule
        let narrowed = stub(
            scratch.path(),
            &serde_json::json!({"permission": {}}).to_string(),
            0,
            0,
        );
        assert!(prove(narrowed, &["1.18.15"]).is_err(), "a missing rule");

        // no permission at all — what OPENCODE_DISABLE_PROJECT_CONFIG would do
        // to a resolution that had nowhere else to read one from
        let bare = stub(scratch.path(), "{}", 0, 0);
        assert!(prove(bare, &["1.18.15"]).is_err(), "no permission");

        // unreadable output, and a refusing binary
        let garbage = stub(scratch.path(), "not json", 0, 0);
        assert!(prove(garbage, &["1.18.15"]).is_err(), "unreadable");
        let failing = stub(
            scratch.path(),
            &serde_json::json!({"permission": expected}).to_string(),
            3,
            0,
        );
        assert!(prove(failing, &["1.18.15"]).is_err(), "non-zero exit");

        // a version Paseo resolves that this posture was never proved against
        let good = stub(
            scratch.path(),
            &serde_json::json!({"permission": expected}).to_string(),
            0,
            0,
        );
        assert!(
            prove(good.clone(), &["1.19.0"]).is_err(),
            "an unproven version"
        );

        // a binary that does not answer in time
        let slow = stub(
            scratch.path(),
            &serde_json::json!({"permission": expected}).to_string(),
            0,
            5,
        );
        assert!(prove(slow, &["1.18.15"]).is_err(), "a timeout");

        // and a binary that is not there at all
        assert!(
            prove(scratch.path().join("absent"), &["1.18.15"]).is_err(),
            "an unrunnable binary"
        );
    }

    #[test]
    fn the_preserved_roots_are_bridged_rather_than_assumed_equal() {
        let provider = ProviderBinary {
            path: PathBuf::from("/opt/homebrew/bin/opencode"),
            version: "1.18.15".to_owned(),
            auth: Some(PathBuf::from("~/.local/share/opencode/auth.json")),
        };
        prove_preserved_roots(&provider, Path::new("/Users/igor/.local/share/opencode"))
            .expect("the data root holds the credentials the daemon names");

        assert!(
            prove_preserved_roots(&provider, Path::new("/somewhere/else")).is_err(),
            "a different data root means a different seat"
        );
    }
}
