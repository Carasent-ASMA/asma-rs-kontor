//! Claude's consultation tool boundary, evaluated before every tool call.
//!
//! This hook grants reads and the registry's scoped consultation operations.
//! Everything else is denied immediately, including planning transitions,
//! delegation and shell execution. Unknown tools never inherit ambient grants.

use std::io::{self, Read};

use kontor_mcp::ServeProfile;

fn permitted(tool: &str) -> bool {
    matches!(tool, "Read" | "Glob" | "Grep" | "ToolSearch")
        || tool.strip_prefix("mcp__kontor__").is_some_and(|name| {
            ServeProfile::find("consultation").is_some_and(|profile| profile.tools.contains(&name))
        })
}

pub(crate) fn run() -> std::process::ExitCode {
    // A broken/oversized hook request must deny, never silently allow.
    let mut bytes = Vec::new();
    let allowed = io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .is_ok()
        && bytes.len() <= 1024 * 1024
        && serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .is_some_and(|event| {
                event["hook_event_name"] == "PreToolUse"
                    && event["tool_name"].as_str().is_some_and(permitted)
            });
    println!(
        "{}",
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": if allowed { "allow" } else { "deny" },
                "permissionDecisionReason": "Consultation permits file reads and scoped Kontor findings only."
            }
        })
    );
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_are_allowed_but_writes_delegation_and_plan_approval_are_denied() {
        for tool in [
            "Read",
            "Glob",
            "Grep",
            "ToolSearch",
            "mcp__kontor__kontor_committee_findings_record",
            "mcp__kontor__kontor_committee_run_get",
            "mcp__kontor__kontor_advisor_run_settle",
        ] {
            assert!(permitted(tool), "{tool}");
        }
        for tool in [
            "Bash",
            "Write",
            "Edit",
            "NotebookEdit",
            "Agent",
            "Task",
            "Skill",
            "EnterPlanMode",
            "ExitPlanMode",
            "AskUserQuestion",
            "mcp__paseo__create_agent",
            "mcp__kontor__kontor_gate_record",
            "mcp__foreign__kontor_committee_findings_record",
            "mcp__kontor__kontor_committee_findings_record_extra",
            "UnknownTool",
            "",
        ] {
            assert!(!permitted(tool), "{tool}");
        }
    }
}
