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

/// Resolve one binding, allocate one Committee jointly, or place one
/// `planning_pair@1` pair, against the activated bundle.
///
/// The request names exactly one mode. Single mode takes `binding_key` and the
/// top-level eligibility, and answers the `FleetSelection`. Joint mode takes
/// `allocation` alone — each slot states its own eligibility (J-02) — and
/// answers the joint selection from one verified snapshot. Planning pair mode
/// takes `planning_pair` alone (PP-03, PP-04) and answers the
/// `PlanningPairPlacement`. `binding_key` with `allocation`, or no mode at
/// all, is J-01 exactly as before the planning pair mode existed.
///
/// The answer is the envelope every command prints: `status` 200 and the
/// result verbatim as the body. A result with no eligible route, or no
/// complete joint allocation, is the defined block result; it is printed in
/// full, but as a refusal — `status` 409, `placement_blocked` — so no caller
/// can mistake it for a route to launch. Anything unverifiable is refused
/// before a result exists.
fn fleet_policy_resolve(
    tool: &'static ToolSpec,
    state_root: &Path,
    arguments: &serde_json::Value,
) -> ExitClass {
    let present = |name: &str| arguments.get(name).is_some_and(|value| !value.is_null());
    if present("planning_pair") {
        if present("allocation") || present("binding_key") {
            return output::emit_local(
                tool.name,
                "invalid_request",
                rule::PP03,
                "name exactly one of --binding-key, --allocation and --planning-pair",
            );
        }
        if present("unavailable_accounts") || present("excluded_vendors") {
            return output::emit_local(
                tool.name,
                "invalid_request",
                rule::PP04,
                "state each member's eligibility inside --planning-pair",
            );
        }
        return fleet_policy_planning_pair(tool, state_root, &arguments["planning_pair"]);
    }
    let joint = present("allocation");
    if joint == present("binding_key") {
        return output::emit_local(
            tool.name,
            "invalid_request",
            rule::J01,
            "name either --binding-key or --allocation, not both and not neither",
        );
    }
    if joint {
        if present("unavailable_accounts") || present("excluded_vendors") {
            return output::emit_local(
                tool.name,
                "invalid_request",
                rule::J02,
                "state each slot's eligibility inside --allocation",
            );
        }
        return fleet_policy_allocate(tool, state_root, &arguments["allocation"]);
    }
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
        Err(error) => return refuse(tool, &error),
    };
    let Ok(document) = serde_json::to_value(&selection) else {
        output::note("the selection could not be rendered as JSON");
        return ExitClass::Unexpected;
    };
    answer(
        tool,
        selection.selected.is_some(),
        document,
        "selection",
        "no route in the bound chain is eligible under the stated eligibility",
    )
}

/// Joint mode: every slot from one verified snapshot, through the one
/// allocator the daemon's Committee admission also uses.
fn fleet_policy_allocate(
    tool: &'static ToolSpec,
    state_root: &Path,
    allocation: &serde_json::Value,
) -> ExitClass {
    let Ok(request) = serde_json::from_value::<kontor_fleet_activation::JointAllocationRequest>(
        allocation.clone(),
    ) else {
        return output::emit_local(
            tool.name,
            "invalid_request",
            "the allocation is not a joint allocation request",
            "send diversity and the ordered slots the schema declares",
        );
    };
    let selection = match kontor_fleet_activation::allocate(state_root, &request) {
        Ok(selection) => selection,
        Err(error) => return refuse(tool, &error),
    };
    let Ok(document) = serde_json::to_value(&selection) else {
        output::note("the allocation could not be rendered as JSON");
        return ExitClass::Unexpected;
    };
    answer(
        tool,
        selection.is_complete(),
        document,
        "allocation",
        "no complete allocation gives every slot an eligible route under the diversity rule",
    )
}

/// Planning pair mode: both members from one verified snapshot, through the
/// shared reader's `place_planning_pair`, which is a joint allocation under
/// distinct actual vendors and nothing else.
///
/// The answer is placement evidence. It carries the protocol, the one
/// allocator receipt, its placement hash and the frozen members; it has no
/// verdict and satisfies no gate.
fn fleet_policy_planning_pair(
    tool: &'static ToolSpec,
    state_root: &Path,
    planning_pair: &serde_json::Value,
) -> ExitClass {
    let Ok(request) = serde_json::from_value::<kontor_fleet_activation::PlanningPairRequest>(
        planning_pair.clone(),
    ) else {
        return output::emit_local(
            tool.name,
            "invalid_request",
            "the planning_pair is not a planning pair request",
            "send the two members, seat-a then seat-b, the schema declares",
        );
    };
    let placement = match kontor_fleet_activation::place_planning_pair(state_root, &request) {
        Ok(placement) => placement,
        Err(error) => return refuse(tool, &error),
    };
    let Ok(document) = serde_json::to_value(&placement) else {
        output::note("the placement could not be rendered as JSON");
        return ExitClass::Unexpected;
    };
    answer(
        tool,
        placement.is_complete(),
        document,
        "placement",
        "no complete placement gives both planning pair members an eligible route on distinct actual vendors",
    )
}

/// Print one result: 200 with the body verbatim, or the defined block result
/// under 409 `placement_blocked`.
fn answer(
    tool: &'static ToolSpec,
    placed: bool,
    document: serde_json::Value,
    key: &str,
    message: &str,
) -> ExitClass {
    let envelope = if placed {
        Envelope {
            tool: tool.name.to_owned(),
            status: 200,
            body: document,
        }
    } else {
        let mut body = serde_json::json!({
            "code": "placement_blocked",
            "message": message,
        });
        body[key] = document;
        Envelope {
            tool: tool.name.to_owned(),
            status: 409,
            body,
        }
    };
    output::emit(&envelope)
}

/// One verification refusal, as the CLI's local refusal document.
fn refuse(tool: &'static ToolSpec, error: &FleetError) -> ExitClass {
    match error {
        FleetError::Invalid { rule } if ABSENT.contains(rule) => output::emit_local(
            tool.name,
            "not_found",
            rule,
            "activate a bundle whose policy binds this key, or name a key it binds",
        ),
        FleetError::Invalid { rule } => output::emit_local(
            tool.name,
            "invalid_request",
            rule,
            "repair or re-activate the state root's fleet activation, or correct the request; nothing past the first failed check was read",
        ),
        FleetError::Read { .. } => output::emit_local(
            tool.name,
            "unavailable",
            "the activated fleet policy could not be read in the state root",
            "check the state root and retry; nothing was resolved",
        ),
        _ => output::emit_local(
            tool.name,
            "invalid_request",
            "the activated fleet policy is not a valid schema_version 1 or 2 document",
            "repair or re-activate the state root's fleet activation",
        ),
    }
}
