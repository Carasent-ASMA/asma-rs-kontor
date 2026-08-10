//! Contract tests for the `kontor` CLI scaffold (KON-MVP-02).
//!
//! The scaffold binary supports exactly one behavior: `--version` prints one
//! `kontor <version>` line and exits immediately without binding sockets or
//! spawning children.

use assert_cmd::Command;

#[test]
fn version_is_one_line_and_exits_immediately() {
    let mut cmd = Command::cargo_bin("kontor").unwrap();
    let out = cmd.arg("--version").output().unwrap();

    assert!(out.status.success(), "kontor --version must exit 0");
    let stdout = String::from_utf8(out.stdout).expect("version output must be UTF-8");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "version output must be exactly one line");
    assert!(
        lines[0].starts_with("kontor "),
        "version line must start with 'kontor ', got: {lines:?}"
    );
}
