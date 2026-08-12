//! Assembling the one client this CLI is allowed to have.
//!
//! The transport itself is `kontor_mcp::client`, re-exported below: the CLI and the
//! MCP server talk to a Realm the same way, over `/v1`, on loopback, with one tier
//! secret read from the Realm's own `0600` file. This module is only the part that
//! is a *command-line* concern — turning global flags into a state root, an
//! endpoint and a tier.
//!
//! There is nothing here that opens a database, spawns a process or learns a runtime
//! endpoint, and no code path that could: the crate graph below this one is
//! `kontor-mcp` and `kontor-core`, and neither reaches any of those.

use std::path::PathBuf;

pub use kontor_mcp::client::{
    CallFailure, CallerTier, Credential, Endpoint, HttpTransport, LocalError, RealmClient, Refusal,
    Request, Transport,
};

/// The environment variable a state root may be named in.
pub const STATE_ROOT_ENV: &str = "KONTOR_STATE_ROOT";

/// The environment variable a base URL may be named in.
pub const BASE_URL_ENV: &str = "KONTOR_BASE_URL";

/// The state root a command acts against.
///
/// The flag wins, then the environment. There is deliberately no *default*: the
/// daemon takes its state root explicitly too, and a CLI that guessed one would
/// sooner or later read a different Realm's credential file than the daemon a caller
/// had running — which is the mistake `realm_mismatch` exists to catch, arrived at
/// through carelessness rather than through a real conflict.
///
/// The environment is read here rather than through clap's `env` attribute, which
/// needs a clap feature this workspace does not pin (CON-007).
///
/// # Errors
/// Returns [`LocalError::Invalid`] when no state root was named at all.
pub fn state_root(named: Option<&PathBuf>) -> Result<PathBuf, LocalError> {
    if let Some(named) = named {
        return Ok(named.clone());
    }
    std::env::var_os(STATE_ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(LocalError::Invalid {
            subject: "state root",
            rule: "name the realm's state root with --state-root or KONTOR_STATE_ROOT".to_owned(),
        })
}

/// The base URL a command should call, when one was named.
#[must_use]
pub fn base_url(named: Option<&str>) -> Option<String> {
    named.map(ToOwned::to_owned).or_else(|| {
        std::env::var(BASE_URL_ENV)
            .ok()
            .filter(|value| !value.is_empty())
    })
}

/// Build the client one command will use.
///
/// The tier is the operation's own requirement unless the caller insisted on
/// another, so an ordinary read presents the observer secret and never the admin
/// one. Reaching for the strongest available credential by default is how a CLI
/// makes its own tier model decorative.
///
/// # Errors
/// Returns [`LocalError`] when the state root was not named, the credential file is
/// missing or unreadable, the recorded endpoint is not a document this build wrote,
/// or the resolved base URL is not loopback.
pub fn connect(
    named_root: Option<&PathBuf>,
    named_base_url: Option<&str>,
    tier: CallerTier,
) -> Result<RealmClient, LocalError> {
    let root = state_root(named_root)?;
    let endpoint = Endpoint::resolve(&root, base_url(named_base_url).as_deref())?;
    let credential = Credential::read(&root, tier)?;
    let transport = HttpTransport::new(endpoint, credential)?;
    Ok(RealmClient::new(Box::new(transport)))
}
