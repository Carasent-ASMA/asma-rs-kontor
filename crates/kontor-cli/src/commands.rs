//! From a parsed command line to an exit code.
//!
//! # One path, and where each refusal comes from
//!
//! ```text
//! parse            → clap                       ⇒ exit 2 on a syntax error
//! resolve locally  → state root, endpoint, tier  ⇒ exit 2 on a local problem
//! gate + validate  → kontor_mcp::execute         ⇒ exit 3 / 2, nothing dispatched
//! call the daemon  → /v1                         ⇒ the daemon's own code
//! render           → output                      ⇒ exactly one JSON value
//! ```
//!
//! The interesting property is the third line: an authority refusal and an operand
//! refusal both happen before a request exists, so `kontor --authority observer run
//! launch …` cannot reach the daemon at all. That is a stronger statement than "the
//! daemon would have said no", and it is the one a test can check by counting what a
//! recording transport received.
//!
//! Local problems are reported on standard error with **no JSON on standard
//! output**. That keeps the promise that a document on standard output came from a
//! Realm: a missing credential file is this machine's problem, and inventing a
//! realm-shaped envelope for it would be inventing a Realm that never answered.

use kontor_mcp::capability::Gate;
use kontor_mcp::client::CallerTier;
use kontor_mcp::server::KontorMcp;

use crate::args::{Authority, Cli, Command, Invocation};
use crate::output::{self, ExitClass};

/// Run one parsed command line.
///
/// Returns the exit class rather than exiting, so the whole path is testable and
/// `main` stays the three lines a binary owns.
pub async fn run(cli: &Cli) -> ExitClass {
    match &cli.command {
        Command::Mcp { serve_as } => serve(cli, *serve_as).await,
        _ => match cli.invocation() {
            // Unreachable: every arm but `Mcp` names an operation, and the test
            // `every_subcommand_names_an_operation_the_catalogue_serves` holds it
            // to that.
            None => {
                output::note("that command names no operation");
                ExitClass::Unexpected
            }
            Some(invocation) => perform(cli, &invocation).await,
        },
    }
}

/// Perform one catalogue operation.
async fn perform(cli: &Cli, invocation: &Invocation) -> ExitClass {
    // The operation's own requirement, unless the caller insisted otherwise. An
    // operation this build does not serve is named as such here rather than after a
    // credential has been read for it.
    let Some(tool) = kontor_mcp::tools::find(invocation.operation) else {
        output::note(format!(
            "`{}` is not an operation this build serves",
            invocation.operation
        ));
        return ExitClass::Absent;
    };
    let tier = invocation.authority.unwrap_or(tool.tier);

    let client =
        match crate::client::connect(cli.state_root.as_ref(), cli.base_url.as_deref(), tier) {
            Ok(client) => client,
            Err(error) => {
                output::note(error);
                return ExitClass::Local;
            }
        };
    perform_with(&client, tier, invocation).await
}

/// Perform one operation against an already-built client.
///
/// Split out from [`perform`] so the whole path after connecting — the gate, the
/// catalogue, the envelope and the exit class — runs against a recording fake
/// without a socket or a daemon (TST-001). `connect` is the only part left out, and
/// it is the part that has nothing to do with the contract.
pub async fn perform_with(
    client: &kontor_mcp::client::RealmClient,
    tier: CallerTier,
    invocation: &Invocation,
) -> ExitClass {
    match kontor_mcp::execute(
        client,
        Gate::new(tier),
        invocation.operation,
        &invocation.operands,
    )
    .await
    {
        Ok(envelope) => output::emit(&envelope),
        Err(failure) => report(&failure, client.expected_realm().as_deref()),
    }
}

/// Report one failure in the shape its kind deserves.
fn report(failure: &kontor_mcp::Failure, realm_id: Option<&str>) -> ExitClass {
    use kontor_mcp::Failure;
    use kontor_mcp::client::CallFailure;

    // A local misconfiguration never produces a document: nothing about it came
    // from a Realm, so nothing about it belongs in a realm-qualified envelope.
    if let Failure::Call(CallFailure::Local(local)) = failure {
        output::note(local);
        return ExitClass::Local;
    }
    if let Failure::Call(CallFailure::Transport(transport)) = failure {
        output::note(transport);
        return ExitClass::Unavailable;
    }
    let code = failure.code().to_owned();
    let class = output::emit_refusal(&failure.body(realm_id), &code);
    // A hint, only at a prompt, and only for the refusals where the next step is
    // not obvious from the code alone.
    if output::interactive()
        && let Some(hint) = hint(&code)
    {
        output::note(hint);
    }
    class
}

/// What to do next, for the refusals whose answer is a specific action.
fn hint(code: &str) -> Option<&'static str> {
    match code {
        "revision_conflict" => {
            Some("the aggregate moved: read it again and retry with the revision the body reports")
        }
        "timeline_refetch_required" => {
            Some("read the timeline again from the start; this does not mean the run ended")
        }
        "resnapshot_required" => Some(
            "the position is outside the retained history: snapshot again and resume from there",
        ),
        "reconciliation_pending" => Some(
            "startup reconciliation has not finished, so scheduling is still shut; retry shortly",
        ),
        "forbidden" => Some("this operation needs a higher realm authority than was presented"),
        "idempotency_conflict" => Some(
            "that key already committed a different command: use a fresh key, or repeat the original request exactly",
        ),
        _ => None,
    }
}

/// Serve the tool surface over stdio.
async fn serve(cli: &Cli, serve_as: Authority) -> ExitClass {
    let tier = CallerTier::from(serve_as);
    let client =
        match crate::client::connect(cli.state_root.as_ref(), cli.base_url.as_deref(), tier) {
            Ok(client) => client,
            Err(error) => {
                output::note(error);
                return ExitClass::Local;
            }
        };
    // Standard output is the protocol channel from here on, so every diagnostic —
    // including this one — goes to standard error.
    output::note(format!(
        "serving the kontor tool surface over stdio at {tier} authority"
    ));
    match kontor_mcp::server::serve_stdio(KontorMcp::new(client)).await {
        Ok(()) => ExitClass::Success,
        Err(error) => {
            output::note(format!("the mcp server stopped: {error}"));
            ExitClass::Unexpected
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_with_a_specific_next_step_has_a_hint_and_the_rest_do_not() {
        // A hint is for a refusal where the code alone does not say what to do. A
        // hint on `not_found` would just be noise.
        for code in [
            "revision_conflict",
            "timeline_refetch_required",
            "resnapshot_required",
            "reconciliation_pending",
            "forbidden",
            "idempotency_conflict",
        ] {
            assert!(hint(code).is_some(), "{code} needs a next step");
        }
        for code in ["not_found", "unauthenticated", "unavailable", "teapot"] {
            assert!(hint(code).is_none(), "{code} does not need prose");
        }
    }
}
