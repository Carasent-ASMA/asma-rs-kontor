//! `kontor-mcp` — serve one Kontor realm's tool surface over stdio.
//!
//! # What the arguments may and may not say
//!
//! A seat is three facts: which realm's state root holds the credentials, which
//! tier of that realm this process acts at, and — only when the realm is not on
//! its default port — where it listens. There is deliberately no argument that
//! carries a bearer value, names a non-loopback address or selects a free-form
//! tool subset: the credential comes from the realm's own `0600` file, the
//! address is validated as loopback whatever produced it, and the tool list
//! follows from the tier. `--serve-profile` selects a *registry-declared*
//! narrowing of that list — presentation only; authority remains the credential
//! tier's, and a profile can never widen it.
//!
//! A secret is never printed and never passed on argv, where it would be visible
//! in every process listing on the machine.

use std::path::PathBuf;

use clap::Parser;
use kontor_mcp::{CallerTier, KontorMcp, ServeProfile, connect, serve_stdio};

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

    /// Serve only the named registry-declared profile's subset of the tier's
    /// tools.
    ///
    /// Profiles narrow presentation only — authority remains the credential
    /// tier's, and a profile never widens it. Free-form tool lists are
    /// deliberately not accepted: the valid names are declared in the registry,
    /// next to the tiers, so there is exactly one authority model.
    #[arg(long, value_name = "NAME")]
    serve_profile: Option<String>,

    /// Where the realm listens, when it is not on its default loopback port.
    ///
    /// Read from the realm's own endpoint file when omitted. A non-loopback
    /// address is refused here, before anything is dispatched.
    #[arg(long, value_name = "URL")]
    base_url: Option<String>,
}

/// The registry profile a `--serve-profile` argument names, or a refusal that
/// lists the valid names.
fn resolve_profile(name: &str) -> Result<&'static ServeProfile, String> {
    ServeProfile::find(name).ok_or_else(|| {
        format!(
            "no serve profile named `{name}`; the valid profiles are: {}",
            ServeProfile::names().join(", ")
        )
    })
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = Args::parse();
    let tier = match CallerTier::parse(&args.credential_tier) {
        Ok(tier) => tier,
        Err(error) => return fail(&error),
    };
    // An unknown profile fails here, before the protocol starts, for the same
    // reason a bad tier does: a misconfigured seat should die with a message on
    // standard error rather than serve a surface nobody chose.
    let profile = match args.serve_profile.as_deref().map(resolve_profile) {
        None => None,
        Some(Ok(profile)) => Some(profile),
        Some(Err(error)) => return fail(&error),
    };
    // Everything local is resolved before the protocol starts, so a misconfigured
    // seat fails with a message on standard error rather than as a tool refusal a
    // client would have to interpret.
    let dispatcher = match connect(&args.state_root, args.base_url.as_deref(), tier) {
        Ok(dispatcher) => dispatcher,
        Err(error) => return fail(&error),
    };
    let dispatcher = match profile {
        Some(profile) => dispatcher.with_profile(profile),
        None => dispatcher,
    };
    eprintln!(
        "kontor-mcp: serving {} at {tier} authority{}",
        dispatcher.base_url(),
        profile.map_or_else(String::new, |profile| format!(
            " under the `{}` serve profile",
            profile.name
        ))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST-004: an unknown profile is refused before serving, and the refusal
    /// names the profiles that do exist.
    #[test]
    fn an_unknown_profile_is_refused_and_the_valid_names_are_listed() {
        let error = resolve_profile("no-such-profile").expect_err("not a declared profile");
        assert!(
            error.contains("no-such-profile"),
            "the refusal names what was asked for: {error}"
        );
        assert!(
            error.contains("worker"),
            "the refusal lists the valid profiles: {error}"
        );
        assert!(
            error.contains("consultation"),
            "the refusal lists every valid profile: {error}"
        );
    }

    #[test]
    fn a_declared_profile_resolves() {
        let profile = resolve_profile("worker").expect("the worker profile is declared");
        assert_eq!(profile.name, "worker");
        let profile =
            resolve_profile("consultation").expect("the consultation profile is declared");
        assert_eq!(profile.name, "consultation");
    }
}
