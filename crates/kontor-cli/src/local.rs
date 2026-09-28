//! The registry's local operations: in-process handlers run before any
//! connection exists.
//!
//! A local operation is declared in the same registry as every routed one —
//! the same name, tier and argument schema, and the same generated command —
//! but it has no `/v1` route. `main` branches on that declared class before it
//! connects, and this module runs the one handler the row names against
//! `--state-root`, with no daemon, base URL, credential or network. The tier is
//! admitted by the same gate and the arguments are validated by the same rules
//! the dispatcher applies to a routed call, so a local command cannot be a
//! looser way in.
//!
//! The only handler reads through `kontor-fleet-activation`, the daemon-free
//! verified reader the daemon's own placement path uses (ASMA-8280 B-1): the
//! files it trusts, their guards and the order it verifies them in cannot
//! differ between the two.

use std::path::Path;

use kontor_fleet::{Eligibility, FleetError};
use kontor_fleet_activation::rule;
use kontor_mcp::{CallerTier, Envelope, Gate, LocalOperation, ToolSpec};

use crate::output::{self, ExitClass};

/// The refusals that mean "nothing the activation names answers this", rather
/// than an activation or artifact that failed a check.
const ABSENT: &[&str] = &[
    rule::A08,
    rule::A10,
    rule::M07,
    rule::C07,
    rule::D01,
    rule::D03,
];

/// Admit, validate and run one local operation, printing exactly one document.
#[must_use]
pub(crate) fn run(
    tool: &'static ToolSpec,
    operation: LocalOperation,
    tier: CallerTier,
    state_root: &Path,
    arguments: &serde_json::Value,
) -> ExitClass {
    if let Err(denied) = Gate::new(tier).admit(tool.name, tool.required_tier(arguments)) {
        return output::emit_local(
            tool.name,
            denied.code(),
            &denied.to_string(),
            denied.action(),
        );
    }
    if let Err(denied) = kontor_mcp::validate(tool, arguments) {
        return output::emit_local(
            tool.name,
            denied.code(),
            &denied.to_string(),
            denied.action(),
        );
    }
    match operation {
        LocalOperation::FleetPolicyResolve => fleet_policy_resolve(tool, state_root, arguments),
    }
}

/// Resolve one binding against the activated bundle and choose under the
/// stated eligibility.
///
/// The answer is the envelope every command prints: `status` 200 and the
/// `FleetSelection` verbatim as the body. A selection with no eligible route
/// is the defined block result; it is printed in full, but as a refusal —
/// `status` 409, `placement_blocked` — so no caller can mistake it for a route
/// to launch. Anything unverifiable is refused before a selection exists.
fn fleet_policy_resolve(
    tool: &'static ToolSpec,
    state_root: &Path,
    arguments: &serde_json::Value,
) -> ExitClass {
    let binding_key = arguments
        .get("binding_key")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let set = |name: &str| {
        arguments
            .get(name)
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let eligibility = Eligibility {
        unavailable_accounts: set("unavailable_accounts"),
        excluded_vendors: set("excluded_vendors"),
    };
    let selection = match kontor_fleet_activation::resolve(state_root, binding_key, &eligibility) {
        Ok(selection) => selection,
        Err(FleetError::Invalid { rule }) if ABSENT.contains(&rule) => {
            return output::emit_local(
                tool.name,
                "not_found",
                rule,
                "activate a bundle whose policy binds this key, or name a key it binds",
            );
        }
        Err(FleetError::Invalid { rule }) => {
            return output::emit_local(
                tool.name,
                "invalid_request",
                rule,
                "repair or re-activate the state root's fleet activation; nothing past the first failed check was read",
            );
        }
        Err(FleetError::Read { .. }) => {
            return output::emit_local(
                tool.name,
                "unavailable",
                "the activated fleet policy could not be read in the state root",
                "check the state root and retry; nothing was resolved",
            );
        }
        Err(_) => {
            return output::emit_local(
                tool.name,
                "invalid_request",
                "the activated fleet policy is not a valid schema_version 1 or 2 document",
                "repair or re-activate the state root's fleet activation",
            );
        }
    };
    let Ok(document) = serde_json::to_value(&selection) else {
        output::note("the selection could not be rendered as JSON");
        return ExitClass::Unexpected;
    };
    let envelope = if selection.selected.is_some() {
        Envelope {
            tool: tool.name.to_owned(),
            status: 200,
            body: document,
        }
    } else {
        Envelope {
            tool: tool.name.to_owned(),
            status: 409,
            body: serde_json::json!({
                "code": "placement_blocked",
                "message": "no route in the bound chain is eligible under the stated eligibility",
                "selection": document,
            }),
        }
    };
    output::emit(&envelope)
}
