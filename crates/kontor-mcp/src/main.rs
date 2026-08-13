//! `kontor-mcp` — serve one Kontor realm's tool surface over stdio.
//!
//! # What the arguments may and may not say
//!
//! A seat is three facts: which realm's state root holds the credentials, which
//! tier of that realm this process acts at, and — only when the realm is not on
//! its default port — where it listens. There is deliberately no argument that
//! carries a bearer value, names a non-loopback address or selects a tool subset:
//! the credential comes from the realm's own `0600` file, the address is validated
//! as loopback whatever produced it, and the tool list follows from the tier.
//!
//! A secret is never printed and never passed on argv, where it would be visible
//! in every process listing on the machine.

use std::path::PathBuf;

use clap::Parser;
use kontor_mcp::{CallerTier, KontorMcp, connect, serve_stdio};

/// Serve one Kontor realm over the Model Context Protocol.
#[derive(Debug, Parser)]
#[command(name = "kontor-mcp", version, about, long_about = None)]
struct Args {
    /// The realm's state root: the directory holding its credential file.
    #[arg(long, value_name = "PATH")]
    state_root: PathBuf,

    /// Which of the realm's three secrets this process acts with.
    #[arg(long, value_name = "TIER")]
    credential_tier: String,

    /// Where the realm listens, when it is not on its default loopback port.
    ///
    /// Read from the realm's own endpoint file when omitted. A non-loopback
    /// address is refused here, before anything is dispatched.
    #[arg(long, value_name = "URL")]
    base_url: Option<String>,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = Args::parse();
    let tier = match CallerTier::parse(&args.credential_tier) {
        Ok(tier) => tier,
        Err(error) => return fail(&error),
    };
    // Everything local is resolved before the protocol starts, so a misconfigured
    // seat fails with a message on standard error rather than as a tool refusal a
    // client would have to interpret.
    let dispatcher = match connect(&args.state_root, args.base_url.as_deref(), tier) {
        Ok(dispatcher) => dispatcher,
        Err(error) => return fail(&error),
    };
    eprintln!(
        "kontor-mcp: serving {} at {tier} authority",
        dispatcher.base_url()
    );
    match serve_stdio(KontorMcp::new(dispatcher)).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => fail(&*error),
    }
}

/// Report a startup failure on standard error and exit non-zero.
///
/// Standard output belongs to the protocol: a diagnostic written there would be
/// read as a malformed MCP frame by the client that is waiting on it.
fn fail(error: &dyn std::fmt::Display) -> std::process::ExitCode {
    eprintln!("kontor-mcp: {error}");
    std::process::ExitCode::FAILURE
}
