//! Claude's consultation tool boundary, evaluated before every tool call.
//!
//! This hook grants reads and one registry profile's scoped operations.
//! Everything else is denied immediately, including planning transitions,
//! delegation and shell execution. Unknown tools never inherit ambient grants.
//!
//! # Which profile a guard enforces
//!
//! The profile is selected on the command line, and only two are closed
//! enough to guard:
//!
//! * `--consultation-tool-guard` alone is the Advisor and Committee
//!   consultation surface, exactly as it always was;
//! * `--consultation-tool-guard --serve-profile <name>` names the profile
//!   explicitly, and the only accepted names are `consultation` and
//!   `planning_pair_member` (ASMA-8282 D-3).
//!
//! Any other argument list — an unknown profile, a missing or repeated value,
//! an extra argument — is malformed, and a malformed guard denies every tool,
//! file reads included. It never falls back to another profile and never
//! unions two: a planning pair member's guard permits no consultation tool and
//! a consultation guard permits no member tool. A planning pair member guard
//! whose registry profile is absent, or no longer exactly the domain's closed
//! member surface, denies every tool too.

use std::io::{self, Read};

use kontor_core::planning_pair::{MEMBER_MCP_TOOLS, MEMBER_SERVE_PROFILE};
use kontor_mcp::ServeProfile;

/// The largest hook request the guard will read; anything larger is denied.
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// The only MCP server whose tools a guard can permit.
const KONTOR_SERVER_PREFIX: &str = "mcp__kontor__";

/// The closed profile one guard process enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuardProfile {
    /// The Advisor and Committee consultation surface.
    Consultation,
    /// A planning pair member's three operations.
    PlanningPairMember,
}

impl GuardProfile {
    /// The guard's arguments after `--consultation-tool-guard`, or `None`
    /// when they are malformed.
    fn select(rest: &[String]) -> Option<Self> {
        match rest {
            [] => Some(Self::Consultation),
            [flag, name] if flag == "--serve-profile" => match name.as_str() {
                "consultation" => Some(Self::Consultation),
                name if name == MEMBER_SERVE_PROFILE => Some(Self::PlanningPairMember),
                _ => None,
            },
            _ => None,
        }
    }

    /// The registry tools this profile permits, or `None` when the registry
    /// no longer declares the profile this guard was written for.
    fn tools(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Consultation => ServeProfile::find("consultation").map(|profile| profile.tools),
            Self::PlanningPairMember => ServeProfile::find(MEMBER_SERVE_PROFILE)
                .map(|profile| profile.tools)
                .filter(|tools| *tools == MEMBER_MCP_TOOLS.as_slice()),
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::Consultation => {
                "Consultation permits file reads and scoped Kontor findings only."
            }
            Self::PlanningPairMember => {
                "A planning pair member may read files and record only its own finding and answer."
            }
        }
    }
}

/// Why a malformed guard denied.
const MALFORMED: &str =
    "This consultation guard was given an unknown or malformed profile, so it denies every tool.";

fn permitted(tool: &str, profile: GuardProfile) -> bool {
    let Some(tools) = profile.tools() else {
        return false;
    };
    matches!(tool, "Read" | "Glob" | "Grep" | "ToolSearch")
        || tool
            .strip_prefix(KONTOR_SERVER_PREFIX)
            .is_some_and(|name| tools.contains(&name))
}

/// The decision for one hook request under the guard's arguments.
fn decide(rest: &[String], request: &[u8]) -> (bool, &'static str) {
    let Some(profile) = GuardProfile::select(rest) else {
        return (false, MALFORMED);
    };
    let allowed = request.len() <= MAX_REQUEST_BYTES
        && serde_json::from_slice::<serde_json::Value>(request)
            .ok()
            .is_some_and(|event| {
                event["hook_event_name"] == "PreToolUse"
                    && event["tool_name"]
                        .as_str()
                        .is_some_and(|tool| permitted(tool, profile))
            });
    (allowed, profile.reason())
}

/// Run the guard with the arguments that followed `--consultation-tool-guard`.
pub(crate) fn run(rest: &[String]) -> std::process::ExitCode {
    // A broken/oversized hook request must deny, never silently allow.
    let mut bytes = Vec::new();
    let read = io::stdin()
        .take(u64::try_from(MAX_REQUEST_BYTES).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut bytes)
        .is_ok();
    let (allowed, reason) = if read {
        decide(rest, &bytes)
    } else {
        (
            false,
            GuardProfile::select(rest).map_or(MALFORMED, GuardProfile::reason),
        )
    };
    println!(
        "{}",
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": if allowed { "allow" } else { "deny" },
                "permissionDecisionReason": reason
            }
        })
    );
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn event(tool: &str) -> Vec<u8> {
        serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": tool})
            .to_string()
            .into_bytes()
    }

    fn allows(rest: &[&str], tool: &str) -> bool {
        decide(&args(rest), &event(tool)).0
    }

    const MEMBER: &[&str] = &["--serve-profile", "planning_pair_member"];

    /// Everything a seat's harness offers that no guard may permit.
    const DENIED: &[&str] = &[
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
        "UnknownTool",
        "",
    ];

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
            assert!(allows(&[], tool), "{tool}");
        }
        for tool in DENIED.iter().copied().chain([
            "mcp__foreign__kontor_committee_findings_record",
            "mcp__kontor__kontor_committee_findings_record_extra",
        ]) {
            assert!(!allows(&[], tool), "{tool}");
        }
    }

    /// The omitted profile is the legacy consultation surface, and naming it
    /// explicitly is the same surface: neither grants a member tool.
    #[test]
    fn the_omitted_and_the_explicit_consultation_profile_are_the_one_legacy_surface() {
        for tool in MEMBER_MCP_TOOLS {
            let name = format!("mcp__kontor__{tool}");
            assert!(!allows(&[], &name), "{name}");
            assert!(
                !allows(&["--serve-profile", "consultation"], &name),
                "{name}"
            );
        }
        for tool in ServeProfile::find("consultation")
            .expect("the consultation profile")
            .tools
        {
            let name = format!("mcp__kontor__{tool}");
            assert_eq!(
                allows(&[], &name),
                allows(&["--serve-profile", "consultation"], &name),
                "{name}"
            );
        }
        assert_eq!(
            decide(&[], &event("Read")).1,
            "Consultation permits file reads and scoped Kontor findings only.",
            "the legacy reason is unchanged"
        );
    }

    /// A member guard permits file reads and exactly the three member tools
    /// under the exact `kontor` server prefix, and nothing else: no
    /// consultation, caller, Advisor or Committee tool, no foreign server
    /// carrying a member tool's name, and no near spelling.
    #[test]
    fn a_member_guard_permits_exactly_the_three_member_tools() {
        for tool in ["Read", "Glob", "Grep", "ToolSearch"] {
            assert!(allows(MEMBER, tool), "{tool}");
        }
        for tool in MEMBER_MCP_TOOLS {
            assert!(allows(MEMBER, &format!("mcp__kontor__{tool}")), "{tool}");
        }
        for tool in DENIED.iter().copied().chain([
            "mcp__kontor__kontor_committee_findings_record",
            "mcp__kontor__kontor_advisor_run_settle",
            "mcp__kontor__kontor_committee_run_get",
            "mcp__kontor__kontor_planning_pair_run_invoke",
            "mcp__kontor__kontor_planning_pair_clarification_request",
            "mcp__kontor__kontor_planning_pair_disposition_record",
            "mcp__kontor__kontor_planning_pair_profile_apply",
            "mcp__foreign__kontor_planning_pair_run_get",
            "mcp__kontorx__kontor_planning_pair_run_get",
            "mcp_kontor__kontor_planning_pair_run_get",
            "mcp__kontor__kontor_planning_pair_run_get_",
            "mcp__kontor__kontor_planning_pair_findings_records",
            "mcp__kontor__KONTOR_PLANNING_PAIR_RUN_GET",
            "mcp__Kontor__kontor_planning_pair_run_get",
            "kontor_planning_pair_run_get",
            "mcp__kontor__",
        ]) {
            assert!(!allows(MEMBER, tool), "{tool}");
        }
        assert_eq!(
            decide(&args(MEMBER), &event("Read")).1,
            "A planning pair member may read files and record only its own finding and answer."
        );
    }

    /// Only the two closed profiles are accepted. An unknown profile, a
    /// profile the registry serves but the guard does not close over, and any
    /// malformed argument list deny every tool, file reads included.
    #[test]
    fn an_unknown_or_malformed_guard_profile_denies_every_tool() {
        for rest in [
            &["--serve-profile", "worker"][..],
            &["--serve-profile", "leadership"],
            &["--serve-profile", "planning_pair_members"],
            &["--serve-profile", "Planning_Pair_Member"],
            &["--serve-profile", ""],
            &["--serve-profile"],
            &["planning_pair_member"],
            &[
                "--serve-profile",
                "planning_pair_member",
                "--serve-profile",
                "consultation",
            ],
            &["--serve-profile", "planning_pair_member", "extra"],
            &["--serve-profile=planning_pair_member"],
            &["--profile", "planning_pair_member"],
        ] {
            for tool in ["Read", "mcp__kontor__kontor_planning_pair_run_get"] {
                let (allowed, reason) = decide(&args(rest), &event(tool));
                assert!(!allowed, "{rest:?} must not allow {tool}");
                assert_eq!(reason, MALFORMED, "{rest:?}");
            }
        }
    }

    /// An oversized or unparseable request, or another hook event, is denied.
    #[test]
    fn an_oversized_malformed_or_foreign_hook_request_is_denied() {
        let mut oversized = event("Read");
        oversized.resize(MAX_REQUEST_BYTES + 1, b' ');
        for request in [
            oversized,
            b"not json".to_vec(),
            Vec::new(),
            serde_json::json!({"hook_event_name": "PostToolUse", "tool_name": "Read"})
                .to_string()
                .into_bytes(),
            serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": 7})
                .to_string()
                .into_bytes(),
        ] {
            for rest in [&[][..], MEMBER] {
                assert!(!decide(&args(rest), &request).0, "{rest:?}");
            }
        }
        // The size bound is inclusive: a request of exactly the bound is read.
        let mut exact = event("Read");
        exact.resize(MAX_REQUEST_BYTES, b' ');
        assert!(decide(&args(MEMBER), &exact).0);
    }

    /// The member guard is closed over the domain's member surface itself: it
    /// is the registry profile, and that profile is exactly the three tools.
    #[test]
    fn the_member_guard_is_generated_from_the_one_closed_member_surface() {
        assert_eq!(
            GuardProfile::PlanningPairMember.tools(),
            Some(MEMBER_MCP_TOOLS.as_slice())
        );
    }
}
