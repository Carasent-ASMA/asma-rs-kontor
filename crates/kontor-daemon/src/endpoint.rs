//! Where this Realm is listening, recorded so a local caller does not have to guess.
//!
//! # Why this file exists
//!
//! A Realm's state root already holds everything a local caller needs except one
//! fact: the port. The credential file is there, the database is there — but a daemon
//! started with `--port 9312` is invisible to a CLI that can only assume the default.
//! The alternatives were to make every command carry `--base-url`, or to let the
//! caller guess and report a confusing "nothing is listening". Recording the bound
//! address next to the credentials it belongs with is smaller than either.
//!
//! # Why the *bound* address and not the configured one
//!
//! It is written after the socket is bound, from the listener's own local address.
//! A daemon configured with `--port 0` is asking the operating system to choose, and
//! the configured value is `0` — a number no caller can connect to. What is recorded
//! has to be what a caller can actually reach.
//!
//! # Why a failure here does not stop the daemon
//!
//! The file is a convenience, not a claim of ownership: the lock is what proves this
//! process owns the state root, and the credential file is what authenticates a
//! caller. A read-only state root should not stop a Realm from serving — it should
//! mean local callers name `--base-url` themselves.

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The file a Realm's loopback endpoint is recorded in.
///
/// The spelling is shared with `kontor_mcp::client::ENDPOINT_FILE`, which is the
/// reader. The two are held together by `the_recorded_endpoint_is_what_a_client_reads`
/// below and by the corresponding check in that crate's protocol suite.
pub const ENDPOINT_FILE: &str = "endpoint.json";

/// The document generation this build writes.
const ENDPOINT_SCHEMA: u32 = 1;

/// The recorded endpoint.
#[derive(Debug, Serialize)]
struct StoredEndpoint<'a> {
    /// The document generation, so a later format is refused rather than misread.
    schema_version: u32,
    /// The loopback base URL, with no trailing slash.
    base_url: &'a str,
}

/// The path the endpoint is recorded at inside `state_root`.
#[must_use]
pub fn path_in(state_root: &Path) -> PathBuf {
    state_root.join(ENDPOINT_FILE)
}

/// Record the address a caller can reach this Realm at.
///
/// # Errors
/// Returns the underlying failure when the file cannot be written. Callers are
/// expected to warn and carry on: see the module docs for why this is not fatal.
pub fn publish(state_root: &Path, bound: SocketAddr) -> std::io::Result<PathBuf> {
    let path = path_in(state_root);
    // An IPv6 literal needs its brackets in a URL authority, which is what
    // `SocketAddr`'s own rendering already produces.
    let base_url = format!("http://{bound}");
    let document = serde_json::to_vec_pretty(&StoredEndpoint {
        schema_version: ENDPOINT_SCHEMA,
        base_url: &base_url,
    })
    .map_err(std::io::Error::other)?;
    // Written through a temporary file and renamed, so a caller reading concurrently
    // sees either the previous endpoint or this one and never half a document.
    let temporary = state_root.join(format!("{ENDPOINT_FILE}.{}.partial", std::process::id()));
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(&document)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recorded_endpoint_is_what_a_client_reads() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        let bound: SocketAddr = "127.0.0.1:9312".parse().expect("a socket address");
        let path = publish(directory.path(), bound).expect("the endpoint is recorded");
        assert_eq!(path, directory.path().join("endpoint.json"));

        let bytes = std::fs::read(&path).expect("the file is readable");
        let document: serde_json::Value = serde_json::from_slice(&bytes).expect("the file is JSON");
        assert_eq!(document["schema_version"], serde_json::json!(1));
        assert_eq!(
            document["base_url"],
            serde_json::json!("http://127.0.0.1:9312"),
            "the recorded base url is one a client can call verbatim"
        );
    }

    #[test]
    fn an_ipv6_endpoint_keeps_the_brackets_a_url_authority_needs() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        let bound: SocketAddr = "[::1]:7717".parse().expect("a socket address");
        let path = publish(directory.path(), bound).expect("the endpoint is recorded");
        let bytes = std::fs::read(&path).expect("the file is readable");
        let document: serde_json::Value = serde_json::from_slice(&bytes).expect("the file is JSON");
        assert_eq!(
            document["base_url"],
            serde_json::json!("http://[::1]:7717"),
            "without the brackets this would not parse as a URL at all"
        );
    }

    #[test]
    fn publishing_twice_replaces_rather_than_appends() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        publish(
            directory.path(),
            "127.0.0.1:1111".parse().expect("a socket address"),
        )
        .expect("the first endpoint is recorded");
        let path = publish(
            directory.path(),
            "127.0.0.1:2222".parse().expect("a socket address"),
        )
        .expect("the second endpoint is recorded");
        let bytes = std::fs::read(&path).expect("the file is readable");
        let document: serde_json::Value = serde_json::from_slice(&bytes).expect("the file is JSON");
        assert_eq!(
            document["base_url"],
            serde_json::json!("http://127.0.0.1:2222"),
            "a restart on another port must not leave the old one readable"
        );
        // And the temporary file is gone, not left beside it.
        let leftovers: Vec<_> = std::fs::read_dir(directory.path())
            .expect("the directory is readable")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains("partial"))
            .collect();
        assert!(leftovers.is_empty(), "no partial file is left behind");
    }
}
