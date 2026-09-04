//! The command surface, built from the MCP tool registry rather than beside it.
//!
//! # Why there is no command table here
//!
//! The CLI and the MCP server expose the same operations at the same authorities
//! with the same arguments. Writing that down twice would mean two lists that agree
//! until someone edits one, and the one that drifts is always the one with fewer
//! tests. So every subcommand below is generated from [`kontor_mcp::REGISTRY`]:
//! adding a tool adds a command, and a command that named a route the registry does
//! not have cannot be written at all.
//!
//! The only thing this module decides is *spelling*. `kontor_epic_apply` is a fine
//! tool name and a poor command name, so the prefix is dropped and the underscores
//! become hyphens — mechanically, in [`command_name`], which is a pure function
//! over the registry rather than a second naming decision per command.

use clap::{Arg, ArgAction, ArgMatches, Command};
use kontor_mcp::{ArgType, REGISTRY, ToolSpec};

/// Give one generated name the `'static` lifetime clap's builder requires.
///
/// The command tree is built once per process from a fixed-size registry, so the
/// leak is bounded by the tool count and lasts exactly as long as the process that
/// needs it. Interning into a `OnceLock` would return the same memory with more
/// code and no benefit at this size.
fn leak(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

/// The command spelling of one tool name.
///
/// `kontor_epic_apply` becomes `epic-apply`, which is what a caller types after
/// `kontor`. Repeating the binary's own name in every subcommand would be noise.
#[must_use]
pub(crate) fn command_name(tool: &str) -> String {
    tool.strip_prefix("kontor_")
        .unwrap_or(tool)
        .replace('_', "-")
}

/// The flag spelling of one argument name.
#[must_use]
fn flag_name(argument: &str) -> String {
    argument.replace('_', "-")
}

/// The whole command tree.
#[must_use]
pub(crate) fn build() -> Command {
    let root = Command::new("kontor")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Operate one Kontor realm over its loopback contract.")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("state_root")
                .long("state-root")
                .value_name("PATH")
                // Global rather than required: clap refuses to make a global
                // argument required, and a per-subcommand copy would be 29 copies.
                // `main` refuses the call when it is absent, before connecting.
                .global(true)
                .help("The realm's state root: the directory holding its credential file."),
        )
        .arg(
            Arg::new("tier")
                .long("tier")
                .value_name("TIER")
                .global(true)
                .default_value("observer")
                .help(
                    "Which of the realm's secrets to act with. Defaults to observer, so a \
                     command that mutates has to say so.",
                ),
        )
        .arg(
            Arg::new("base_url")
                .long("base-url")
                .value_name("URL")
                .global(true)
                .help("Where the realm listens, when it is not on its default loopback port."),
        );

    REGISTRY
        .iter()
        .fold(root, |root, tool| root.subcommand(subcommand(tool)))
}

/// One tool's subcommand, with one flag per declared argument.
fn subcommand(tool: &'static ToolSpec) -> Command {
    let command = Command::new(leak(command_name(tool.name))).about(tool.about);
    tool.args.iter().fold(command, |command, argument| {
        command.arg(
            Arg::new(argument.name)
                .long(leak(flag_name(argument.name)))
                .value_name(value_name(argument.ty))
                .required(argument.required)
                .action(ArgAction::Set)
                .help(argument.about),
        )
    })
}

/// What one argument's value looks like in `--help`.
const fn value_name(ty: ArgType) -> &'static str {
    match ty {
        ArgType::Revision | ArgType::SpecVersion | ArgType::U32 | ArgType::U64 | ArgType::I64 => {
            "N"
        }
        ArgType::Bool => "true|false",
        ArgType::TextArray | ArgType::ObjectArray(_) => "JSON_ARRAY",
        ArgType::Json => "JSON",
        ArgType::Timestamp => "RFC3339",
        _ => "VALUE",
    }
}

/// The tool one set of matches names, if any.
#[must_use]
pub(crate) fn resolve(matches: &ArgMatches) -> Option<(&'static ToolSpec, &ArgMatches)> {
    let (name, sub) = matches.subcommand()?;
    let tool = REGISTRY
        .iter()
        .find(|tool| command_name(tool.name) == name)?;
    Some((tool, sub))
}

/// Turn one subcommand's matches into the argument object the dispatcher takes.
///
/// The conversion is by declared type, so `--expected-revision 7` becomes the
/// number `7` rather than the string `"7"` — the dispatcher validates against the
/// same declaration, and a CLI that sent everything as text would be refused by it.
///
/// # Errors
/// Returns the flag and the rule when a value is not the shape its type declares.
/// Nothing is dispatched in that case.
pub(crate) fn arguments(
    tool: &ToolSpec,
    matches: &ArgMatches,
) -> Result<serde_json::Value, String> {
    let mut object = serde_json::Map::new();
    for argument in tool.args {
        let Some(raw) = matches.get_one::<String>(argument.name) else {
            continue;
        };
        let value = convert(argument.ty, raw)
            .map_err(|rule| format!("--{} is not valid: {rule}", flag_name(argument.name)))?;
        object.insert(argument.name.to_owned(), value);
    }
    Ok(serde_json::Value::Object(object))
}

/// Convert one text value into the JSON shape its declared type calls for.
fn convert(ty: ArgType, raw: &str) -> Result<serde_json::Value, String> {
    match ty {
        ArgType::Revision | ArgType::SpecVersion | ArgType::U32 | ArgType::U64 => raw
            .parse::<u64>()
            .map(serde_json::Value::from)
            .map_err(|_| "a non-negative integer".to_owned()),
        ArgType::I64 => raw
            .parse::<i64>()
            .map(serde_json::Value::from)
            .map_err(|_| "an integer".to_owned()),
        ArgType::Bool => raw
            .parse::<bool>()
            .map(serde_json::Value::from)
            .map_err(|_| "true or false".to_owned()),
        // A nested document is given as JSON, because there is no shell spelling of
        // a task graph that is not just JSON with more steps.
        ArgType::TextArray | ArgType::Json | ArgType::Object(_) | ArgType::ObjectArray(_) => {
            serde_json::from_str(raw).map_err(|_| "a JSON document".to_owned())
        }
        // Everything else is text the dispatcher validates against the domain.
        _ => Ok(serde_json::Value::String(raw.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_exactly_one_subcommand_and_no_command_is_invented() {
        let command = build();
        let subcommands: Vec<_> = command
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect();
        assert_eq!(
            subcommands.len(),
            REGISTRY.len(),
            "the command surface and the tool registry are the same list"
        );
        for tool in REGISTRY {
            assert!(
                subcommands.contains(&command_name(tool.name).as_str()),
                "{} has no command",
                tool.name
            );
        }
    }

    #[test]
    fn the_command_spelling_is_mechanical() {
        assert_eq!(command_name("kontor_epic_apply"), "epic-apply");
        assert_eq!(command_name("kontor_realm_get"), "realm-get");
        assert_eq!(
            command_name("kontor_session_permission_respond"),
            "session-permission-respond"
        );
    }

    #[test]
    fn the_tree_is_valid_and_every_flag_matches_its_declared_argument() {
        let mut command = build();
        command.build();
        for tool in REGISTRY {
            let sub = command
                .find_subcommand(command_name(tool.name))
                .unwrap_or_else(|| panic!("{} has a subcommand", tool.name));
            for argument in tool.args {
                let found = sub
                    .get_arguments()
                    .find(|declared| declared.get_id() == argument.name)
                    .unwrap_or_else(|| {
                        panic!("{} is missing --{}", tool.name, flag_name(argument.name))
                    });
                assert_eq!(
                    found.is_required_set(),
                    argument.required,
                    "{}'s --{} disagrees with the registry about being required",
                    tool.name,
                    flag_name(argument.name)
                );
            }
        }
    }

    #[test]
    fn a_declared_number_reaches_the_dispatcher_as_a_number() {
        let command = build();
        let matches = command
            .try_get_matches_from([
                "kontor",
                "--state-root",
                "/tmp/realm",
                "--tier",
                "operator",
                "lifecycle-transition",
                "--project-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70",
                "--epic-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b71",
                "--idempotency-key",
                "close-1",
                "--action",
                "close_epic",
                "--expected-revision",
                "7",
                "--reason",
                "the work is done",
            ])
            .expect("a well-formed command line");
        let (tool, sub) = resolve(&matches).expect("the lifecycle tool");
        assert_eq!(tool.name, "kontor_lifecycle_transition");
        let arguments = arguments(tool, sub).expect("well-formed arguments");
        assert_eq!(arguments["expected_revision"], serde_json::json!(7));
        assert_eq!(arguments["action"], serde_json::json!("close_epic"));
        assert!(
            arguments.get("task_id").is_none(),
            "an omitted optional flag is absent rather than null"
        );
    }

    #[test]
    fn committee_re_review_is_exposed_by_the_generated_cli_command() {
        let command = build();
        let matches = command
            .try_get_matches_from([
                "kontor",
                "--state-root",
                "/tmp/realm",
                "--tier",
                "operator",
                "committee-run-invoke",
                "--project-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70",
                "--epic-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b71",
                "--idempotency-key",
                "committee-re-review-1",
                "--profile",
                r#"{"id":"independent_review","version":1}"#,
                "--question",
                "Verify the governed remediation.",
                "--caller-seat-binding-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b72",
                "--expected-revision",
                "7",
                "--re-review",
                r#"{"completion_round":1,"completion_revision":9,"failed_committee_run_id":"01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b73","failed_result_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","remediation_hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","remediation_integration_receipt":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}"#,
            ])
            .expect("the generated CLI accepts the re-review route");
        let (tool, sub) = resolve(&matches).expect("the Committee invoke tool");
        let arguments = arguments(tool, sub).expect("the re-review payload is JSON");
        assert_eq!(
            arguments["re_review"]["completion_round"],
            serde_json::json!(1)
        );
    }

    #[test]
    fn committee_initial_recovery_profiles_are_exposed_by_the_admin_cli_command() {
        let command = build();
        let matches = command
            .try_get_matches_from([
                "kontor",
                "--state-root",
                "/tmp/realm",
                "--tier",
                "admin",
                "committee-run-invoke",
                "--project-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70",
                "--epic-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b71",
                "--idempotency-key",
                "committee-initial-recovery-1",
                "--profile",
                r#"{"id":"independent_review","version":1}"#,
                "--question",
                "Verify the governed remediation.",
                "--caller-seat-binding-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b72",
                "--expected-revision",
                "7",
                "--initial-recovery-profiles",
                r#"[{"role_slot_id":"reviewer-b","ordered_routes":[{"provider":"opencode","model":"deepseek/deepseek-v4-flash","effort":"max"}]}]"#,
            ])
            .expect("the generated Admin CLI accepts initial recovery profiles");
        let (tool, sub) = resolve(&matches).expect("the Committee invoke tool");
        let arguments = arguments(tool, sub).expect("the recovery profiles are JSON");
        assert_eq!(
            arguments["initial_recovery_profiles"][0]["role_slot_id"],
            "reviewer-b"
        );
        assert_eq!(
            arguments["initial_recovery_profiles"][0]["ordered_routes"][0]["provider"],
            "opencode"
        );
    }

    #[test]
    fn native_less_committee_reroute_is_exposed_by_the_admin_cli_command() {
        let matches = build()
            .try_get_matches_from([
                "kontor",
                "--state-root",
                "/tmp/realm",
                "--tier",
                "admin",
                "committee-seat-reroute-unmaterialized",
                "--project-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70",
                "--committee-run-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b71",
                "--seat-binding-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b72",
                "--idempotency-key",
                "native-less-reroute-1",
                "--expected-revision",
                "1",
                "--expected-occupancy-generation",
                "1",
                "--expected-model-route",
                r#"{"provider":"opencode","model":"deepseek/deepseek-v4-flash","effort":"max"}"#,
                "--reason",
                "permission_mode_unsupported",
                "--recovery-profile",
                r#"[{"provider":"claude-work","model":"claude-opus-5","effort":"xhigh"}]"#,
            ])
            .expect("the generated Admin CLI accepts a native-less reroute");
        let (tool, sub) = resolve(&matches).expect("the reroute tool");
        let arguments = arguments(tool, sub).expect("the reroute payload is JSON");
        assert_eq!(arguments["expected_occupancy_generation"], 1);
        assert_eq!(arguments["recovery_profile"][0]["provider"], "claude-work");
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_refused_before_anything_is_dispatched() {
        let command = build();
        let matches = command
            .try_get_matches_from([
                "kontor",
                "--state-root",
                "/tmp/realm",
                "epic-get",
                "--project-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70",
                "--epic-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b71",
            ])
            .expect("a well-formed command line");
        let (tool, sub) = resolve(&matches).expect("the epic tool");
        assert!(arguments(tool, sub).is_ok());

        // A JSON-shaped argument that is not JSON is a local failure.
        assert!(convert(ArgType::Json, "not json").is_err());
        assert!(convert(ArgType::Revision, "seven").is_err());
        assert!(convert(ArgType::Bool, "yes").is_err());
    }

    #[test]
    fn a_declared_object_reaches_the_dispatcher_as_an_object() {
        let command = build();
        let matches = command
            .try_get_matches_from([
                "kontor",
                "--state-root",
                "/tmp/realm",
                "--tier",
                "admin",
                "seat-replace",
                "--project-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70",
                "--agent-run-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b71",
                "--role-slot",
                "builder",
                "--expected-predecessor-revision",
                "18446744073709551615",
                "--expected-task-revision",
                "2",
                "--binding-generation",
                "1",
                "--model-route",
                r#"{"provider":"codex","model":"gpt-5.6-sol","effort":"xhigh"}"#,
                "--unavailable-provider",
                r#"{"runtime_binding_id":"01890000-0000-7000-8000-0000000000b1","native_id":"native-claude-1","provider":"claude"}"#,
                "--idempotency-key",
                "replace-1",
            ])
            .expect("a well-formed command line");
        let (tool, sub) = resolve(&matches).expect("the seat replacement tool");
        let arguments = arguments(tool, sub).expect("well-formed arguments");
        assert_eq!(
            arguments["expected_predecessor_revision"],
            serde_json::json!(u64::MAX),
            "the generated CLI must preserve the full unsigned revision domain"
        );
        assert_eq!(
            arguments["model_route"],
            serde_json::json!({
                "provider": "codex",
                "model": "gpt-5.6-sol",
                "effort": "xhigh"
            })
        );
        assert_eq!(
            arguments["unavailable_provider"],
            serde_json::json!({
                "runtime_binding_id": "01890000-0000-7000-8000-0000000000b1",
                "native_id": "native-claude-1",
                "provider": "claude"
            })
        );
    }

    #[test]
    fn a_declared_object_array_reaches_the_dispatcher_as_an_array() {
        let command = build();
        let matches = command
            .try_get_matches_from([
                "kontor",
                "--state-root",
                "/tmp/realm",
                "--tier",
                "admin",
                "consultation-seat-recover",
                "--project-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70",
                "--committee-run-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b71",
                "--seat-binding-id",
                "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b72",
                "--idempotency-key",
                "recover-1",
                "--expected-revision",
                "9",
                "--expected-native-id",
                "native-claude-1",
                "--reason",
                "provider_unavailable",
                "--recovery-profile",
                r#"[{"provider":"codex-work","model":"gpt-5.6-sol","effort":"xhigh"},{"provider":"codex-personal","model":"gpt-5.6-sol","effort":"xhigh"}]"#,
            ])
            .expect("a well-formed command line");
        let (tool, sub) = resolve(&matches).expect("the consultation recovery tool");
        let arguments = arguments(tool, sub).expect("well-formed arguments");
        assert_eq!(
            arguments["recovery_profile"],
            serde_json::json!([
                {
                    "provider": "codex-work",
                    "model": "gpt-5.6-sol",
                    "effort": "xhigh"
                },
                {
                    "provider": "codex-personal",
                    "model": "gpt-5.6-sol",
                    "effort": "xhigh"
                }
            ])
        );
    }

    #[test]
    fn a_missing_required_flag_is_refused_by_the_parser() {
        let command = build();
        assert!(
            command
                .try_get_matches_from(["kontor", "--state-root", "/tmp/realm", "run-get"])
                .is_err(),
            "the run id is required and the parser says so"
        );
    }
}
