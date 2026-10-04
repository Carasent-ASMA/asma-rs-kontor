//! `kontor` — the Kontor control plane command-line interface.
//!
//! One command is one tool is one registered operation — almost always one `/v1`
//! route. The command surface is generated from the MCP tool registry, so a shell
//! and a Paseo session reach the same operations at the same authorities under the
//! same argument names — and the CLI cannot grow a route the tool vocabulary does
//! not have. A row the registry declares *local* has no route: it is run here, by
//! the one in-process handler it names, before any connection exists.
//!
//! The process holds exactly one credential tier, defaulting to `observer`: a
//! command that mutates has to be asked for with `--tier operator` or
//! `--tier admin`, so a careless invocation reads rather than writes.

mod commands;
mod local;
mod output;

use kontor_mcp::{CallerTier, connect};
use output::ExitClass;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(run().await.code())
}

/// Parse, connect, dispatch one tool and print exactly one document.
async fn run() -> ExitClass {
    let matches = commands::build().get_matches();
    let Some((tool, sub)) = commands::resolve(&matches) else {
        output::note("no command was named");
        return ExitClass::Local;
    };

    let Some(state_root) = matches
        .get_one::<String>("state_root")
        .map(std::path::PathBuf::from)
    else {
        output::note("--state-root names the realm to act on, and there is no default");
        return ExitClass::Local;
    };
    let tier = match matches
        .get_one::<String>("tier")
        .map_or(Ok(CallerTier::Observer), |text| CallerTier::parse(text))
    {
        Ok(tier) => tier,
        Err(error) => {
            output::note(error);
            return ExitClass::Local;
        }
    };
    let base_url = matches.get_one::<String>("base_url").map(String::as_str);

    let arguments = match commands::arguments(tool, sub) {
        Ok(arguments) => arguments,
        Err(rule) => {
            return output::emit_local(
                tool.name,
                "invalid_request",
                &rule,
                "correct the arguments and send them again",
            );
        }
    };

    // A local operation branches here, on its declared class, before anything
    // connects: it has no route, so no daemon, base URL or credential is used.
    if let Some(operation) = tool.local() {
        if base_url.is_some() {
            return output::emit_local(
                tool.name,
                "invalid_request",
                "a local operation reads the state root and calls no daemon, so it takes no --base-url",
                "drop --base-url and run it again",
            );
        }
        return local::run(tool, operation, tier, &state_root, &arguments);
    }

    // Everything local resolves before a request exists: a missing credential file
    // or a non-loopback address is this machine's problem, reported as such rather
    // than as a refusal the Realm never issued.
    let dispatcher = match connect(&state_root, base_url, tier) {
        Ok(dispatcher) => dispatcher,
        Err(error) => {
            output::note(&error);
            return output::emit_local(
                tool.name,
                "invalid_request",
                &error.to_string(),
                "fix the local credential or base URL, then retry",
            );
        }
    };

    match dispatcher.call(tool.name, &arguments).await {
        Ok(envelope) => output::emit(&envelope),
        Err(failure) => output::emit_local(
            tool.name,
            failure.code(),
            &failure.to_string(),
            failure.action(),
        ),
    }
}
