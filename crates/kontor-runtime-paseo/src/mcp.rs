//! The Paseo daemon's MCP facade.
//!
//! Paseo has three surfaces, not two. The CLI and the session socket are what
//! every other operation in this adapter speaks, and neither of them can change
//! a workspace's title. The daemon's MCP endpoint can: it serves
//! `rename_workspace`, addressed by workspace id, setting the user-visible title
//! and nothing else.
//!
//! So this module exists for exactly one operation, and it is deliberately not a
//! general MCP client. It calls one tool, reads one answer, and refuses anything
//! it does not recognize — a facade with sixty tools is a large attack surface to
//! borrow for a rename.
//!
//! The endpoint is *derived* from the session endpoint rather than configured
//! separately. They are the same daemon on the same port, and two settings for
//! one process is how a plane ends up renaming workspaces on a host it is not
//! bound to.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use kontor_core::DomainError;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};

/// The tool that sets a workspace's user-visible title.
pub const RENAME_WORKSPACE_TOOL: &str = "rename_workspace";

/// The MCP protocol revision this adapter's calls were recorded against.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// The path the Paseo daemon serves its agent-facing MCP endpoint from.
const MCP_PATH: &str = "/mcp/agents";

/// The largest answer this adapter will read from the facade.
///
/// A rename answers with one short line. Anything larger is not the answer to
/// this call, and reading it into memory first to find that out is how a
/// misconfigured endpoint becomes a memory problem.
const MAX_ANSWER_BYTES: usize = 64 * 1024;

/// The seam between this adapter and the daemon's MCP facade.
///
/// Narrow on purpose: one tool call, one JSON answer. Everything about framing,
/// protocol revisions and transport lives behind it, so the adapter's rename
/// logic is testable without a daemon.
#[async_trait]
pub trait PaseoMcp: Send + Sync + fmt::Debug {
    /// Call one tool and return the structured answer it reported.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the channel failed or the daemon
    /// reported the call as an error. A failure is a fact about the channel and
    /// never about the work: an implementation must not turn one into a success.
    async fn call(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> RuntimeResult<serde_json::Value>;
}

/// The MCP endpoint that belongs to a session endpoint.
///
/// `ws://127.0.0.1:6767/ws` → `http://127.0.0.1:6767/mcp/agents`.
///
/// Deliberately strict. An endpoint this function does not recognize is refused
/// rather than guessed at, because the guess would be a URL this adapter then
/// sends rename commands to.
///
/// # Errors
/// Returns [`RuntimeError::Domain`] when `endpoint` is not a `ws://` or `wss://`
/// URL ending in the session path.
pub fn mcp_endpoint_for(endpoint: &str) -> RuntimeResult<String> {
    let refuse = || {
        RuntimeError::Domain(DomainError::invalid(
            "PaseoEndpoint",
            "is not a Paseo session endpoint this adapter can derive an MCP endpoint from",
        ))
    };
    let (scheme, rest) = match endpoint.split_once("://") {
        Some(("ws", rest)) => ("http", rest),
        Some(("wss", rest)) => ("https", rest),
        _ => return Err(refuse()),
    };
    let authority = rest.strip_suffix("/ws").ok_or_else(refuse)?;
    if authority.is_empty() || authority.contains('/') {
        return Err(refuse());
    }
    Ok(format!("{scheme}://{authority}{MCP_PATH}"))
}

/// A live MCP facade, over HTTP.
///
/// Streamable HTTP with a JSON-RPC body, which is what the daemon serves. The
/// answer arrives as a single server-sent event, so it is parsed as one rather
/// than a stream: this client makes exactly one request/response call and has no
/// use for a session it would have to keep open.
pub struct PaseoMcpHttp {
    endpoint: String,
    client: reqwest::Client,
}

impl fmt::Debug for PaseoMcpHttp {
    /// Written out rather than derived: the endpoint is the whole state worth
    /// seeing, and a derived rendering prints the HTTP client's internals.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PaseoMcpHttp")
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

impl PaseoMcpHttp {
    /// A client for the MCP facade belonging to `session_endpoint`.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] when the session endpoint is not one an
    /// MCP endpoint can be derived from, and [`RuntimeError::Transport`] when the
    /// HTTP client cannot be built.
    pub fn new(session_endpoint: &str, timeout_seconds: u64) -> RuntimeResult<Self> {
        let endpoint = mcp_endpoint_for(session_endpoint)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds.max(1)))
            .build()
            .map_err(|_| RuntimeError::Transport {
                rule: "the MCP facade client could not be built",
            })?;
        Ok(Self { endpoint, client })
    }

    /// The endpoint this client calls.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

#[async_trait]
impl PaseoMcp for PaseoMcpHttp {
    async fn call(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> RuntimeResult<serde_json::Value> {
        let transport = |rule: &'static str| RuntimeError::Transport { rule };
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments },
        });
        let response = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json")
            // Both, and in that order: the facade refuses a client that does not
            // accept the event stream it answers with.
            .header("accept", "application/json, text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|_| transport("the MCP facade could not be reached"))?;
        if !response.status().is_success() {
            return Err(transport("the MCP facade refused the call"));
        }
        let text = response
            .text()
            .await
            .map_err(|_| transport("the MCP facade answer could not be read"))?;
        if text.len() > MAX_ANSWER_BYTES {
            return Err(transport(
                "the MCP facade answered with more than one result",
            ));
        }
        parse_answer(&text)
    }
}

/// The structured result inside one server-sent JSON-RPC answer.
///
/// Kept separate from the transport so the framing rules are testable without a
/// daemon — and they are the part most likely to drift.
///
/// # Errors
/// Returns [`RuntimeError::Transport`] when the body is not one JSON-RPC answer,
/// when it carries a JSON-RPC error, or when the tool reported failure.
pub fn parse_answer(body: &str) -> RuntimeResult<serde_json::Value> {
    let transport = |rule: &'static str| RuntimeError::Transport { rule };
    // One event, one data line. Anything else is not this call's answer.
    let payload = body
        .lines()
        .filter_map(|line| line.strip_prefix("data:").or(Some(line)).map(str::trim))
        .find(|line| line.starts_with('{'))
        .ok_or_else(|| transport("the MCP facade answered with no JSON-RPC payload"))?;
    let answer: serde_json::Value = serde_json::from_str(payload)
        .map_err(|_| transport("the MCP facade answer is not JSON-RPC"))?;
    if answer.get("error").is_some() {
        return Err(transport("the MCP facade reported a JSON-RPC error"));
    }
    let result = answer
        .get("result")
        .ok_or_else(|| transport("the MCP facade answer carries no result"))?;
    if result
        .get("isError")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return Err(transport("the MCP tool reported the call as failed"));
    }
    Ok(result.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_mcp_endpoint_is_derived_from_the_session_endpoint() {
        assert_eq!(
            mcp_endpoint_for("ws://127.0.0.1:6767/ws").expect("a derived endpoint"),
            "http://127.0.0.1:6767/mcp/agents"
        );
        assert_eq!(
            mcp_endpoint_for("wss://paseo.internal:443/ws").expect("a derived endpoint"),
            "https://paseo.internal:443/mcp/agents"
        );
    }

    /// A shape this function does not recognize is refused rather than guessed
    /// at: the guess is a URL rename commands would be sent to.
    #[test]
    fn an_endpoint_this_adapter_does_not_recognize_is_refused() {
        for endpoint in [
            "http://127.0.0.1:6767/ws",
            "ws://127.0.0.1:6767/session",
            "ws://127.0.0.1:6767/a/ws",
            "ws:///ws",
            "127.0.0.1:6767",
            "",
        ] {
            assert!(
                mcp_endpoint_for(endpoint).is_err(),
                "{endpoint} must not derive an MCP endpoint"
            );
        }
    }

    #[test]
    fn a_server_sent_answer_is_read_as_one_result() {
        let result = parse_answer(
            "event: message\ndata: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]},\"jsonrpc\":\"2.0\",\"id\":1}\n\n",
        )
        .expect("the answer is read");
        assert_eq!(result["content"][0]["text"], "ok");
    }

    #[test]
    fn an_error_answer_is_never_read_as_success() {
        for body in [
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32000,\"message\":\"no\"}}",
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"isError\":true,\"content\":[]}}",
            "data: {\"jsonrpc\":\"2.0\",\"id\":1}",
            "not an answer at all",
        ] {
            assert!(
                matches!(parse_answer(body), Err(RuntimeError::Transport { .. })),
                "{body} must not read as a success"
            );
        }
    }
}
