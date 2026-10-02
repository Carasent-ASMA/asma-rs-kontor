//! The shipped guard binary enforces the profile its argument list names.
//!
//! The hook runner only sees the process: its exit status and the decision it
//! prints. These run the built `kontor-mcp` exactly as a composed seat does, so
//! the argument routing in `main` is proved together with the decision. In
//! particular, a malformed guard argument list must still be the guard — exit
//! zero with a deny — and never fall through to the MCP argument parser.

use std::io::Write;
use std::process::{Command, Stdio};

/// Run the guard binary with `args` on one hook request.
fn guard(args: &[&str], request: &serde_json::Value) -> (bool, serde_json::Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kontor-mcp"))
        .arg("--consultation-tool-guard")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the guard starts");
    child
        .stdin
        .take()
        .expect("a stdin")
        .write_all(request.to_string().as_bytes())
        .expect("the request is written");
    let output = child.wait_with_output().expect("the guard exits");
    let reply: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the guard prints one JSON decision");
    (output.status.success(), reply)
}

fn decision(args: &[&str], tool: &str) -> String {
    let (success, reply) = guard(
        args,
        &serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": tool}),
    );
    assert!(success, "the guard always exits zero with a decision");
    assert_eq!(reply["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    reply["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .expect("a decision")
        .to_owned()
}

const MEMBER: &[&str] = &["--serve-profile", "planning_pair_member"];

#[test]
fn the_member_guard_binary_permits_only_the_member_surface() {
    for tool in [
        "Read",
        "mcp__kontor__kontor_planning_pair_run_get",
        "mcp__kontor__kontor_planning_pair_findings_record",
        "mcp__kontor__kontor_planning_pair_answer_record",
    ] {
        assert_eq!(decision(MEMBER, tool), "allow", "{tool}");
    }
    for tool in [
        "Bash",
        "Write",
        "mcp__kontor__kontor_committee_findings_record",
        "mcp__kontor__kontor_planning_pair_disposition_record",
        "mcp__foreign__kontor_planning_pair_run_get",
    ] {
        assert_eq!(decision(MEMBER, tool), "deny", "{tool}");
    }
}

#[test]
fn the_legacy_guard_binary_is_unchanged_and_grants_no_member_tool() {
    assert_eq!(
        decision(&[], "mcp__kontor__kontor_committee_findings_record"),
        "allow"
    );
    assert_eq!(
        decision(&[], "mcp__kontor__kontor_planning_pair_findings_record"),
        "deny"
    );
}

#[test]
fn a_malformed_guard_binary_invocation_denies_and_still_exits_zero() {
    for args in [
        &["--serve-profile", "worker"][..],
        &["--serve-profile"],
        &["--state-root", "/tmp", "--credential-tier", "operator"],
        &["--serve-profile", "planning_pair_member", "extra"],
    ] {
        assert_eq!(decision(args, "Read"), "deny", "{args:?}");
    }
}
