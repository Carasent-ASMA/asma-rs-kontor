//! The `kontor` binary's behaviour before it ever reaches a Realm (KON-MVP-16).
//!
//! Everything asserted here is a fact about *this machine*: a command line that does
//! not parse, a state root that was not named, a credential file that is missing or
//! from another generation, a base URL that is not loopback. None of these calls a
//! daemon — there is none to call, and starting one would break TST-001 — so the
//! binary is run with `assert_cmd`, waits for exit, and leaves nothing behind.
//!
//! The rule the whole file is really about is a distinction between two kinds of
//! local failure:
//!
//! * **This machine is misconfigured** — no state root, no credential file, a base
//!   URL that is not loopback. Nothing about it came from a Realm, so nothing goes on
//!   standard output; inventing a realm-shaped envelope for a missing file would be
//!   inventing a Realm that never answered.
//! * **The request itself is refused, in the contract's own vocabulary** — an
//!   identifier that is not canonical, an authority this caller does not hold. These
//!   *are* `invalid_request` and `forbidden`, and the CLI writes exactly the body the
//!   daemon would have, so a script branching on `code` does not have to know which
//!   side noticed. Nothing is dispatched either way.
//!
//! The mutants this suite exists to kill:
//!
//! * a local misconfiguration reported as a realm refusal, or with a realm-shaped
//!   document on standard output;
//! * a state root silently defaulted, so a caller reads a different Realm than the
//!   daemon it has running;
//! * a non-loopback base URL accepted, which is the one mistake that would take this
//!   control plane off the machine;
//! * a diagnostic written to standard output, breaking every caller that pipes into
//!   `jq`;
//! * `--version` growing a second line, a socket or a child process.

use std::process::Command as StdCommand;

use assert_cmd::Command;
use tempfile::TempDir;

/// A realm state root holding a credential file this build understands.
fn realm() -> TempDir {
    let directory = TempDir::new().expect("a temporary directory");
    std::fs::write(
        directory.path().join("credentials.json"),
        br#"{"schema_version":1,"observer":"o-secret","operator":"p-secret","admin":"a-secret"}"#,
    )
    .expect("the credential fixture is written");
    directory
}

/// Run the binary and return `(code, stdout, stderr)`.
fn run(arguments: &[&str]) -> (i32, String, String) {
    let output = Command::cargo_bin("kontor")
        .expect("the kontor binary is built")
        .args(arguments)
        // Cleared so a developer's own realm cannot make these tests pass or fail.
        .env_remove("KONTOR_STATE_ROOT")
        .env_remove("KONTOR_BASE_URL")
        .output()
        .expect("the binary runs and exits");
    (
        output.status.code().expect("the process exited normally"),
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    )
}

// ---------------------------------------------------------------------------
// The deterministic flags
// ---------------------------------------------------------------------------

#[test]
fn version_is_one_line_and_exits_immediately() {
    let (code, stdout, _stderr) = run(&["--version"]);
    assert_eq!(code, 0, "kontor --version must exit 0");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "version output must be exactly one line");
    assert!(
        lines[0].starts_with("kontor "),
        "version line must start with 'kontor ', got: {lines:?}"
    );
}

#[test]
fn help_names_every_top_level_noun() {
    let (code, stdout, _stderr) = run(&["--help"]);
    assert_eq!(code, 0);
    for noun in [
        "health",
        "events",
        "realm",
        "project",
        "task",
        "gate",
        "mission",
        "run",
        "profile",
        "receipt",
        "runtime",
        "scheduler",
        "session",
        "account",
        "authorize",
        "mcp",
    ] {
        assert!(
            stdout.contains(noun),
            "`{noun}` must be discoverable from --help"
        );
    }
}

// ---------------------------------------------------------------------------
// Command-line errors
// ---------------------------------------------------------------------------

#[test]
fn a_command_line_error_exits_two_and_writes_no_document() {
    for arguments in [
        vec!["not-a-command"],
        vec!["run"],
        vec!["run", "launch"],
        vec!["task", "list"],
        vec!["session", "stream", "r"],
        vec!["gate", "verdict", "--project", "p", "--task", "t"],
        vec!["--authority", "root", "health"],
        vec!["session", "permission", "r", "q", "--decision", "maybe"],
    ] {
        let (code, stdout, stderr) = run(&arguments);
        assert_eq!(code, 2, "{arguments:?} is a command-line error");
        assert!(
            stdout.is_empty(),
            "{arguments:?} must write no document: got {stdout}"
        );
        assert!(
            !stderr.is_empty(),
            "{arguments:?} must say what was wrong on standard error"
        );
    }
}

// ---------------------------------------------------------------------------
// Local configuration errors
// ---------------------------------------------------------------------------

#[test]
fn a_missing_state_root_exits_two_and_says_how_to_name_one() {
    let (code, stdout, stderr) = run(&["health"]);
    assert_eq!(
        code, 2,
        "a state root is never defaulted, because guessing one reads the wrong realm"
    );
    assert!(stdout.is_empty(), "no document, because no realm answered");
    assert!(
        stderr.contains("--state-root") && stderr.contains("KONTOR_STATE_ROOT"),
        "the message must name both ways to fix it: {stderr}"
    );
}

#[test]
fn a_state_root_with_no_credential_file_exits_two_and_names_the_path() {
    let empty = TempDir::new().expect("a temporary directory");
    let (code, stdout, stderr) = run(&["--state-root", empty.path().to_str().unwrap(), "health"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(
        stderr.contains("credentials.json"),
        "the message must name the file that is missing: {stderr}"
    );
    assert!(
        stderr.contains("kontor-daemon"),
        "and what would create it: {stderr}"
    );
}

#[test]
fn a_credential_file_from_another_generation_exits_two_without_echoing_it() {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("credentials.json");
    std::fs::write(
        &path,
        br#"{"schema_version":99,"observer":"o-secret-value","operator":"p","admin":"a"}"#,
    )
    .expect("the fixture is written");

    let (code, stdout, stderr) =
        run(&["--state-root", directory.path().to_str().unwrap(), "health"]);
    assert_eq!(
        code, 2,
        "a generation this build does not understand is refused"
    );
    assert!(stdout.is_empty());
    assert!(
        !stderr.contains("o-secret-value"),
        "a message about a file of secrets must not quote the file: {stderr}"
    );
}

#[test]
fn a_non_loopback_base_url_is_refused_on_this_machine() {
    // The daemon refuses a non-loopback `Host` too. Refusing here as well means a
    // misconfigured caller gets a message about configuration rather than a
    // `forbidden` it has to interpret — and means the request is never sent.
    let directory = realm();
    for base in [
        "http://10.0.0.4:7717",
        "http://kontor.example.com:7717",
        "http://0.0.0.0:7717",
        "http://127.0.0.1.evil.com",
    ] {
        let (code, stdout, stderr) = run(&[
            "--state-root",
            directory.path().to_str().unwrap(),
            "--base-url",
            base,
            "health",
        ]);
        assert_eq!(code, 2, "{base} must be refused before anything is sent");
        assert!(stdout.is_empty(), "{base} must produce no document");
        assert!(
            stderr.contains("loopback"),
            "{base} must be refused for the reason it was refused: {stderr}"
        );
    }
}

#[test]
fn an_unaddressable_base_url_is_refused() {
    let directory = realm();
    for base in ["not a url", "ftp://127.0.0.1", "file:///tmp/kontor"] {
        let (code, stdout, _stderr) = run(&[
            "--state-root",
            directory.path().to_str().unwrap(),
            "--base-url",
            base,
            "health",
        ]);
        assert_eq!(code, 2, "{base} is not an address this client can call");
        assert!(stdout.is_empty());
    }
}

#[test]
fn an_endpoint_file_from_another_generation_is_refused() {
    let directory = realm();
    std::fs::write(
        directory.path().join("endpoint.json"),
        br#"{"schema_version":42,"base_url":"http://127.0.0.1:7717"}"#,
    )
    .expect("the fixture is written");
    let (code, stdout, _stderr) =
        run(&["--state-root", directory.path().to_str().unwrap(), "health"]);
    assert_eq!(
        code, 2,
        "a generation this build does not understand is refused"
    );
    assert!(stdout.is_empty());
}

// ---------------------------------------------------------------------------
// A locally-refused operation never reaches the wire
// ---------------------------------------------------------------------------

#[test]
fn a_malformed_identifier_is_refused_before_any_connection_is_attempted() {
    // There is deliberately no daemon at the configured port. A malformed
    // identifier must therefore still exit 2 rather than 5: the refusal happens
    // before a connection is attempted, so "nothing is listening" is never reached.
    let directory = realm();
    let (code, stdout, stderr) = run(&[
        "--state-root",
        directory.path().to_str().unwrap(),
        // A port nothing is on, so a dispatched request would fail as unavailable.
        "--base-url",
        "http://127.0.0.1:1",
        "run",
        "show",
        "not-a-uuid",
    ]);
    assert_eq!(
        code, 2,
        "a malformed identifier is a caller error and is caught locally, so it is exit 2 and \
         never the exit 5 a dispatched request would have produced: {stderr}"
    );
    // It *does* write a document: this is a refusal in the contract's own vocabulary,
    // and a script branching on `code` should not have to know which side noticed.
    let body: serde_json::Value =
        serde_json::from_str(&stdout).expect("a contract refusal writes one JSON value");
    assert_eq!(
        body["code"],
        serde_json::json!("invalid_request"),
        "the code is the one the daemon would have sent for the same value"
    );
    assert!(
        body["rule"]
            .as_str()
            .is_some_and(|rule| rule.contains("agent_run_id")),
        "and it names the operand that was wrong: {body}"
    );
}

#[test]
fn an_insisted_lower_authority_refuses_a_write_without_a_connection() {
    let directory = realm();
    let (code, stdout, _stderr) = run(&[
        "--state-root",
        directory.path().to_str().unwrap(),
        "--base-url",
        "http://127.0.0.1:1",
        "--authority",
        "observer",
        "run",
        "launch",
        "--project",
        "0192f0c0-0000-7000-8000-000000000001",
        "0192f0c0-0000-7000-8000-000000000002",
        "--expected-revision",
        "1",
    ]);
    assert_eq!(
        code, 3,
        "an authority refusal is exit 3, and it happens without a connection being attempted"
    );
    // This one *does* write a document: it is a refusal in the contract's own
    // vocabulary, shaped exactly like the daemon's own.
    let body: serde_json::Value =
        serde_json::from_str(&stdout).expect("an authority refusal writes one JSON value");
    assert_eq!(body["code"], serde_json::json!("forbidden"));
}

#[test]
fn an_unreachable_realm_is_exit_five_and_not_a_local_error() {
    // Port 1 on loopback: a legal address with nothing behind it. The distinction
    // matters — this is retryable, and a local error is not.
    let directory = realm();
    let (code, stdout, stderr) = run(&[
        "--state-root",
        directory.path().to_str().unwrap(),
        "--base-url",
        "http://127.0.0.1:1",
        "health",
    ]);
    assert_eq!(
        code, 5,
        "no answer from a legal address is unavailable, which is retryable: {stderr}"
    );
    assert!(
        stdout.is_empty(),
        "there is no realm document to print, because no realm answered"
    );
    assert!(
        stderr.contains("kontor-daemon"),
        "the message should suggest the daemon may not be running: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// The output contract
// ---------------------------------------------------------------------------

#[test]
fn a_closed_pipe_is_a_clean_exit() {
    // `kontor … | head -1` closes the read end while the binary is still writing.
    // Dropping the child's stdout handle before it exits reproduces exactly that.
    // The command is one that writes a document without needing a daemon: an
    // authority refusal.
    let directory = realm();
    let binary = assert_cmd::cargo::cargo_bin("kontor");
    let mut child = StdCommand::new(binary)
        .args([
            "--state-root",
            directory.path().to_str().unwrap(),
            "--base-url",
            "http://127.0.0.1:1",
            "--authority",
            "observer",
            "run",
            "launch",
            "--project",
            "0192f0c0-0000-7000-8000-000000000001",
            "0192f0c0-0000-7000-8000-000000000002",
            "--expected-revision",
            "1",
        ])
        .env_remove("KONTOR_STATE_ROOT")
        .env_remove("KONTOR_BASE_URL")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the binary starts");
    // Close the read end before the child writes.
    drop(child.stdout.take());
    let status = child.wait().expect("the binary exits");
    let code = status.code().expect("the process exited normally");
    assert!(
        code == 0 || code == 3,
        "a closed pipe must be a clean exit (0) or the refusal it had already written (3), never a \
         write failure; got {code}"
    );
}

#[test]
fn no_command_writes_a_diagnostic_to_standard_output() {
    // Stated over every failing path this file exercises: a hint on standard output
    // would break every caller that parses it.
    let directory = TempDir::new().expect("a temporary directory");
    for arguments in [
        vec!["health"],
        vec!["--state-root", directory.path().to_str().unwrap(), "health"],
        vec!["not-a-command"],
    ] {
        let (_code, stdout, _stderr) = run(&arguments);
        assert!(
            !stdout.contains("kontor:"),
            "{arguments:?} wrote a diagnostic to standard output: {stdout}"
        );
    }
}
