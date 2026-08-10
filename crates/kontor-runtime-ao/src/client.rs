//! The AO transport: one seam, one live implementation, bounded and quiet.
//!
//! The adapter never speaks HTTP itself. It builds an [`AoCall`], hands it to an
//! [`AoTransport`], and reads an [`AoReply`]. That seam earns its keep three
//! times over:
//!
//! * the contract suite can prove a refusal produced **zero** calls, which is a
//!   claim about the wire that no amount of return-value checking can make;
//! * the acceptance rule "a lost acknowledgement must not cause a second POST"
//!   becomes a count over a recorded ledger instead of an inference;
//! * a fault can be injected *after* the fixture-side effect committed, which is
//!   the one ordering that matters for confirmation-unknown and the one a plain
//!   HTTP mock makes awkward.
//!
//! Two things are deliberately not here. There is no WebSocket client: AO's
//! `/mux` needs an exact workspace-pinned dependency that the root manifest does
//! not carry, and hand-rolled framing to dodge that gate is rejected outright.
//! And no error variant, log line or refusal ever carries a response body, a
//! prompt or a message: AO bodies contain the operator's actual work.

use std::fmt;

use async_trait::async_trait;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};

use crate::wire::{API_V1, MAX_RESPONSE_BYTES};

/// The two HTTP verbs AO's session surface uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AoMethod {
    /// A read.
    Get,
    /// A mutation.
    Post,
}

impl AoMethod {
    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }

    /// Whether this verb can change AO.
    #[must_use]
    pub const fn changes_runtime(self) -> bool {
        matches!(self, Self::Post)
    }
}

impl fmt::Display for AoMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One request the adapter wants made.
///
/// `path` is always absolute and version-pinned; the constructors below are the
/// only way to build one, so no call site can invent an unversioned AO route or
/// interpolate a value into a path without going through this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AoCall {
    /// The verb.
    pub method: AoMethod,
    /// The absolute path, including the `/api/v1` prefix where AO versions it.
    pub path: String,
    /// The query string without its leading `?`, or empty.
    pub query: String,
    /// The JSON request body, for a POST that carries one.
    pub body: Option<String>,
}

impl AoCall {
    /// `GET /healthz` — reachability only.
    #[must_use]
    pub fn healthz() -> Self {
        Self::get("/healthz".to_owned())
    }

    /// `GET /api/v1/agents` — the client catalog.
    #[must_use]
    pub fn agents() -> Self {
        Self::get(format!("{API_V1}/agents"))
    }

    /// `GET /api/v1/projects/{id}` — the pre-spawn project and security check.
    #[must_use]
    pub fn project(project_id: &str) -> Self {
        Self::get(format!("{API_V1}/projects/{}", Self::segment(project_id)))
    }

    /// `GET /api/v1/sessions` — the inventory.
    #[must_use]
    pub fn sessions() -> Self {
        Self::get(format!("{API_V1}/sessions"))
    }

    /// `GET /api/v1/sessions/{id}` — a fresh inspect.
    #[must_use]
    pub fn session(session_id: &str) -> Self {
        Self::get(format!("{API_V1}/sessions/{}", Self::segment(session_id)))
    }

    /// `POST /api/v1/sessions` — spawn.
    #[must_use]
    pub fn spawn(body: String) -> Self {
        Self::post(format!("{API_V1}/sessions"), Some(body))
    }

    /// `POST /api/v1/sessions/{id}/send` — follow-up.
    #[must_use]
    pub fn send(session_id: &str, body: String) -> Self {
        Self::post(
            format!("{API_V1}/sessions/{}/send", Self::segment(session_id)),
            Some(body),
        )
    }

    /// `POST /api/v1/sessions/{id}/kill` — request a stop.
    #[must_use]
    pub fn kill(session_id: &str) -> Self {
        Self::post(
            format!("{API_V1}/sessions/{}/kill", Self::segment(session_id)),
            None,
        )
    }

    /// `POST /api/v1/sessions/{id}/restore` — restart a terminated session.
    #[must_use]
    pub fn restore(session_id: &str) -> Self {
        Self::post(
            format!("{API_V1}/sessions/{}/restore", Self::segment(session_id)),
            None,
        )
    }

    /// `POST /api/v1/sessions/{id}/resume-agent` — restart an exited client.
    #[must_use]
    pub fn resume_agent(session_id: &str) -> Self {
        Self::post(
            format!(
                "{API_V1}/sessions/{}/resume-agent",
                Self::segment(session_id)
            ),
            None,
        )
    }

    /// `GET /api/v1/events?after=<seq>` — durable CDC replay.
    #[must_use]
    pub fn events_after(seq: u64) -> Self {
        Self {
            method: AoMethod::Get,
            path: format!("{API_V1}/events"),
            query: format!("after={seq}"),
            body: None,
        }
    }

    /// The ledger key the contract suite counts calls by: verb and path, never a
    /// body.
    #[must_use]
    pub fn route(&self) -> String {
        if self.query.is_empty() {
            format!("{} {}", self.method, self.path)
        } else {
            format!("{} {}?{}", self.method, self.path, self.query)
        }
    }

    /// Percent-encode one path segment.
    ///
    /// AO session and project ids are opaque foreign strings. Interpolating one
    /// raw would let an id containing `/` or `?` address a different route than
    /// the one the caller named — so anything outside the unreserved set is
    /// encoded rather than trusted.
    fn segment(raw: &str) -> String {
        let mut out = String::with_capacity(raw.len());
        for byte in raw.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    fn get(path: String) -> Self {
        Self {
            method: AoMethod::Get,
            path,
            query: String::new(),
            body: None,
        }
    }

    fn post(path: String, body: Option<String>) -> Self {
        Self {
            method: AoMethod::Post,
            path,
            query: String::new(),
            body,
        }
    }
}

/// One answer from AO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AoReply {
    /// The HTTP status.
    pub status: u16,
    /// The response body.
    pub body: String,
}

impl AoReply {
    /// Build a reply.
    #[must_use]
    pub const fn new(status: u16, body: String) -> Self {
        Self { status, body }
    }

    /// The body of a 2xx answer, deserialized.
    ///
    /// A non-2xx status is a [`RuntimeError::Transport`] naming only the status
    /// class. Its body is read once, to check that it really is AO's locked error
    /// envelope, and then dropped: that envelope carries a human message which may
    /// quote the operator's project path or prompt, and a refusal has to be safe
    /// to log. The check is not ceremony — AO binds a loopback port, so a non-2xx
    /// that is *not* AO's envelope means something other than AO answered, which
    /// is a different problem from AO refusing and must not be reported as one.
    ///
    /// # Errors
    /// * [`RuntimeError::Transport`] — a non-2xx status, or an answer that did not
    ///   come from AO at all.
    /// * [`RuntimeError::Domain`] — a 2xx body that is not this AO envelope.
    pub fn parse<T: serde::de::DeserializeOwned>(&self, subject: &'static str) -> RuntimeResult<T> {
        if !(200..300).contains(&self.status) {
            if serde_json::from_str::<crate::wire::AoApiError>(&self.body).is_err() {
                return Err(RuntimeError::Transport {
                    rule: "answer did not come from an Agent Orchestrator daemon",
                });
            }
            return Err(RuntimeError::Transport {
                rule: match self.status {
                    400..=499 => "runtime refused the request",
                    500..=599 => "runtime failed to answer the request",
                    _ => "runtime answered with an unusable status",
                },
            });
        }
        serde_json::from_str(&self.body).map_err(|_| {
            RuntimeError::Domain(kontor_core::DomainError::invalid(
                subject,
                "is not the AO 0.12.1 envelope this adapter is pinned to",
            ))
        })
    }
}

/// The seam between the adapter's policy and AO's wire.
#[async_trait]
pub trait AoTransport: Send + Sync + fmt::Debug {
    /// Make one call.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the channel failed. That is a
    /// fact about the channel and never about the work: an implementation must
    /// not turn a timeout into an empty success.
    async fn call(&self, call: &AoCall) -> RuntimeResult<AoReply>;
}

/// The live loopback transport.
///
/// Bounded on purpose in three ways: one request timeout, one response-size cap,
/// and no redirect following. AO is a loopback daemon, so a redirect or an
/// unbounded body is a malfunction rather than a case to accommodate.
#[derive(Debug)]
pub struct AoHttpTransport {
    endpoint: url::Url,
    client: reqwest::Client,
}

impl AoHttpTransport {
    /// Build a transport against `endpoint` with `timeout_seconds` per request.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] for an endpoint that is not an absolute
    /// `http`/`https` base, and [`RuntimeError::Transport`] when the HTTP client
    /// cannot be constructed.
    pub fn new(endpoint: &str, timeout_seconds: u64) -> RuntimeResult<Self> {
        let endpoint = url::Url::parse(endpoint).map_err(|_| {
            RuntimeError::Domain(kontor_core::DomainError::invalid(
                "AoEndpoint",
                "is not an absolute URL",
            ))
        })?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            return Err(RuntimeError::Domain(kontor_core::DomainError::invalid(
                "AoEndpoint",
                "must be an http or https base",
            )));
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| RuntimeError::Transport {
                rule: "client could not be constructed",
            })?;
        Ok(Self { endpoint, client })
    }

    fn url(&self, call: &AoCall) -> RuntimeResult<url::Url> {
        let mut url = self
            .endpoint
            .join(&call.path)
            .map_err(|_| RuntimeError::Transport {
                rule: "call could not be addressed against the configured endpoint",
            })?;
        if call.query.is_empty() {
            url.set_query(None);
        } else {
            url.set_query(Some(&call.query));
        }
        Ok(url)
    }
}

#[async_trait]
impl AoTransport for AoHttpTransport {
    async fn call(&self, call: &AoCall) -> RuntimeResult<AoReply> {
        let url = self.url(call)?;
        let mut request = match call.method {
            AoMethod::Get => self.client.get(url),
            AoMethod::Post => self.client.post(url),
        };
        request = request.header("accept", "application/json");
        if let Some(body) = &call.body {
            request = request
                .header("content-type", "application/json")
                .body(body.clone());
        }
        let response = request.send().await.map_err(|_| RuntimeError::Transport {
            rule: "channel failed before the runtime answered",
        })?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|_| RuntimeError::Transport {
            rule: "channel failed while the answer was being read",
        })?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(RuntimeError::Transport {
                rule: "answer exceeded the bounded response size",
            });
        }
        Ok(AoReply::new(status, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_is_version_pinned_and_names_no_body() {
        let calls = [
            AoCall::agents(),
            AoCall::project("p1"),
            AoCall::sessions(),
            AoCall::session("s1"),
            AoCall::spawn("{\"projectId\":\"p1\"}".to_owned()),
            AoCall::send("s1", "{\"message\":\"secret work\"}".to_owned()),
            AoCall::kill("s1"),
            AoCall::restore("s1"),
            AoCall::resume_agent("s1"),
            AoCall::events_after(7),
        ];
        for call in &calls {
            assert!(
                call.path.starts_with(API_V1),
                "{} is not version pinned",
                call.path
            );
            assert!(
                !call.route().contains("secret work"),
                "a call ledger must never carry a body"
            );
        }
        assert_eq!(AoCall::healthz().path, "/healthz");
        assert_eq!(
            AoCall::events_after(7).route(),
            "GET /api/v1/events?after=7"
        );
    }

    #[test]
    fn a_hostile_session_id_cannot_address_another_route() {
        // AO ids are opaque foreign strings. Interpolated raw, `s1/kill` under a
        // send would POST a kill.
        let call = AoCall::send("s1/kill", "{}".to_owned());
        assert_eq!(call.path, "/api/v1/sessions/s1%2Fkill/send");
        let query = AoCall::session("s1?after=1");
        assert_eq!(query.path, "/api/v1/sessions/s1%3Fafter%3D1");
        assert!(query.query.is_empty());
    }

    #[test]
    fn a_non_2xx_from_something_other_than_ao_is_not_ao_refusing() {
        // AO binds a loopback port. A 404 from a stray server that happened to
        // claim the port is not the runtime declining a request, and reporting it
        // as one would send an operator looking in the wrong place.
        let foreign = AoReply::new(404, "<html>Not Found</html>".to_owned());
        assert_eq!(
            foreign
                .parse::<serde_json::Value>("AoTest")
                .expect_err("not an AO answer"),
            RuntimeError::Transport {
                rule: "answer did not come from an Agent Orchestrator daemon"
            }
        );
    }

    #[test]
    fn a_non_2xx_status_is_a_channel_fact_and_carries_no_body() {
        let refused = AoReply::new(
            409,
            "{\"error\":\"conflict\",\"code\":\"X\",\"message\":\"/Users/someone/secret-project\"}"
                .to_owned(),
        );
        let error = refused
            .parse::<serde_json::Value>("AoTest")
            .expect_err("a 409 is not an answer");
        assert_eq!(
            error,
            RuntimeError::Transport {
                rule: "runtime refused the request"
            }
        );
        // The refusal must not have picked up the path AO quoted back.
        assert!(!format!("{error}").contains("secret-project"));
        assert!(!format!("{error:?}").contains("secret-project"));
    }

    #[test]
    fn a_foreign_2xx_body_fails_typed_rather_than_defaulting() {
        let reply = AoReply::new(200, "{\"unexpected\":true}".to_owned());
        let error = reply
            .parse::<crate::wire::AoListSessionsResponse>("AoListSessionsResponse")
            .expect_err("a body without `sessions` is not this envelope");
        assert!(matches!(error, RuntimeError::Domain(_)));
    }

    #[test]
    fn a_post_is_the_only_verb_that_changes_the_runtime() {
        assert!(AoMethod::Post.changes_runtime());
        assert!(!AoMethod::Get.changes_runtime());
    }

    #[test]
    fn an_endpoint_must_be_an_absolute_http_base() {
        assert!(AoHttpTransport::new("http://127.0.0.1:3001", 5).is_ok());
        assert!(AoHttpTransport::new("127.0.0.1:3001", 5).is_err());
        assert!(AoHttpTransport::new("ws://127.0.0.1:3001", 5).is_err());
        assert!(AoHttpTransport::new("file:///tmp", 5).is_err());
    }
}
