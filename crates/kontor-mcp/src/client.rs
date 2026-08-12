//! How a CLI or an MCP tool reaches its Realm, and the only things it is allowed
//! to reach.
//!
//! This module is the whole outward surface of both callers, and it is
//! deliberately narrow in four ways:
//!
//! 1. **One base URL, and it must be loopback.** There is no flag, environment
//!    variable or config field that widens it. A Realm holds the credentials and
//!    transcripts of every run on the machine; the daemon refuses to *bind*
//!    anything but loopback, and a client that would happily talk to a remote
//!    address is the other half of the same mistake.
//! 2. **One tier, chosen once.** A [`HttpTransport`] is built with exactly one
//!    tier secret and cannot be asked for another. That is what makes "the MCP
//!    server runs at one authority" structural rather than a convention a tool
//!    could forget: a tool has no token to reach for.
//! 3. **`/v1` only.** Every path a caller can name is a `/v1/…` route of the
//!    daemon. There is no runtime endpoint here, no `asma` executable, no SQLite
//!    file and no adapter — those live behind the daemon, which is the process
//!    that owns them.
//! 4. **Refusals are relayed, never translated.** A daemon refusal arrives as
//!    [`Refusal`] carrying the `ApiErrorBody` it sent, byte for byte. This crate
//!    reads exactly one field out of it — `code` — to decide an exit class, and
//!    passes the document itself through unchanged.
//!
//! # Two failure kinds that must not be confused
//!
//! A [`Refusal`] is the daemon saying no, and it is *an answer*: it names the
//! Realm, it carries a code from the closed vocabulary, and relaying it is the
//! caller's whole job. A [`TransportFailure`] is the absence of an answer. They
//! map to different exit classes because a caller that retries on the first and
//! gives up on the second has them backwards.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use futures::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

/// The file a Realm's credential set lives in, inside its state root.
///
/// The daemon writes it; this crate only ever reads it. The name is repeated here
/// rather than imported because importing it would mean depending on
/// `kontor-daemon`, and `kontor-daemon` depends on every runtime adapter — which
/// is exactly what a CLI must not link. `the_credential_file_matches_the_daemons`
/// in `tests/protocol.rs` is what keeps the two spellings honest.
pub const CREDENTIAL_FILE: &str = "credentials.json";

/// The file a Realm's loopback endpoint is recorded in, inside its state root.
pub const ENDPOINT_FILE: &str = "endpoint.json";

/// The document generation this build writes and is willing to read.
pub const LOCAL_SCHEMA: u32 = 1;

/// The loopback port a Kontor daemon binds when it is not told otherwise.
pub const DEFAULT_PORT: u16 = 7717;

/// How much of the control plane one caller may reach.
///
/// The three names are the three keys of the credential file and the three tiers
/// of `kontor_api::auth::CallerCapability`. The enum is declared here rather than
/// imported for the reason [`CREDENTIAL_FILE`] is: `kontor-api` reaches SQLite,
/// and a CLI that linked it would be one careless line away from opening a store
/// it has no business opening.
///
/// The ordering is [`CallerTier::rank`] and nothing else, so reordering the
/// variants cannot silently promote a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CallerTier {
    /// Read liveness, identity, snapshots, persisted events and session content.
    Observer,
    /// Everything an observer may do, plus control-plane writes, session messages
    /// and permission responses.
    Operator,
    /// Everything an operator may do, plus account and policy-authority routes.
    Admin,
}

impl CallerTier {
    /// Every tier, lowest authority first.
    pub const ALL: &'static [Self] = &[Self::Observer, Self::Operator, Self::Admin];

    /// The stable spelling, which is also the credential file's key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observer => "observer",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }

    /// The explicit policy rank. Higher reaches more.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Observer => 0,
            Self::Operator => 1,
            Self::Admin => 2,
        }
    }

    /// Whether this tier reaches everything `required` reaches.
    #[must_use]
    pub const fn at_least(self, required: Self) -> bool {
        self.rank() >= required.rank()
    }

    /// Parse the stable spelling.
    ///
    /// # Errors
    /// Returns [`LocalError::UnknownTier`] for anything else. There is no
    /// defaulting: guessing a tier is guessing an authority.
    pub fn parse(text: &str) -> Result<Self, LocalError> {
        Self::ALL
            .iter()
            .copied()
            .find(|tier| tier.as_str() == text)
            .ok_or(LocalError::UnknownTier)
    }
}

impl fmt::Display for CallerTier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Why a caller could not even form a request.
///
/// Every variant is a fact about *this machine* — a missing file, an unreadable
/// document, an address that is not loopback. None of them has reached the daemon,
/// which is why they are reported as a local failure rather than as a refusal the
/// Realm never issued.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LocalError {
    /// The state root does not hold a credential file.
    #[error(
        "no realm credential file at {path}: start `kontor-daemon --state-root` there first, \
         or point --state-root at the realm you mean"
    )]
    NoCredentials {
        /// Where one was looked for.
        path: PathBuf,
    },
    /// A local document could not be read.
    #[error("the local {what} could not be read: {source}")]
    Io {
        /// Which document.
        what: &'static str,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// A local document is not one this build understands.
    ///
    /// The parser's own message is dropped on purpose: a serde error quotes the
    /// offending input, and the offending input here is a file of secrets.
    #[error("the local {what} is not a document this build wrote")]
    Malformed {
        /// Which document.
        what: &'static str,
    },
    /// The configured base URL is not a loopback address.
    #[error("kontor talks to loopback only, and {base_url} is not a loopback address")]
    NotLoopback {
        /// The address that was refused.
        base_url: String,
    },
    /// The configured base URL is not a URL at all.
    #[error("{base_url} is not a base URL")]
    NotAUrl {
        /// The value that was refused.
        base_url: String,
    },
    /// A tier was named that does not exist.
    #[error("a realm authority is one of observer, operator or admin")]
    UnknownTier,
    /// A caller-supplied value is not what the domain accepts.
    #[error("{subject} is not valid: {rule}")]
    Invalid {
        /// What was being parsed.
        subject: &'static str,
        /// The rule that refused.
        rule: String,
    },
    /// The HTTP client itself could not be built.
    #[error("the loopback http client could not be built")]
    Client,
}

impl LocalError {
    /// Refuse a caller-supplied value the domain rejected.
    #[must_use]
    pub fn invalid(subject: &'static str, error: &kontor_core::DomainError) -> Self {
        Self::Invalid {
            subject,
            rule: error.to_string(),
        }
    }
}

/// The on-disk credential set, as this crate reads it.
#[derive(Debug, Deserialize)]
struct StoredCredentials {
    /// The document generation.
    schema_version: u32,
    /// The read-only tier's secret.
    observer: String,
    /// The control-plane-write tier's secret.
    operator: String,
    /// The account- and policy-authority tier's secret.
    admin: String,
}

/// The on-disk record of where a Realm is listening.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEndpoint {
    /// The document generation.
    pub schema_version: u32,
    /// The loopback base URL, with no trailing slash.
    pub base_url: String,
}

/// Where a Realm is, and which of its secrets this caller holds.
///
/// Building one is the only place a secret is read off disk, and the secret does
/// not leave this struct except as an `Authorization` header value.
pub struct Credential {
    tier: CallerTier,
    secret: SecretString,
}

impl fmt::Debug for Credential {
    /// Names the tier only. The secret is a `SecretString`, but a `Debug` that
    /// printed the field name next to it would still invite a future edit to
    /// print the value.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credential")
            .field("tier", &self.tier)
            .finish_non_exhaustive()
    }
}

impl Credential {
    /// Read one tier's secret out of a Realm's `0600` credential file.
    ///
    /// # Errors
    /// Returns [`LocalError::NoCredentials`] when the state root holds no
    /// credential file, [`LocalError::Io`] when it cannot be read, and
    /// [`LocalError::Malformed`] when it is not a credential set this build wrote.
    pub fn read(state_root: &Path, tier: CallerTier) -> Result<Self, LocalError> {
        let path = state_root.join(CREDENTIAL_FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(LocalError::NoCredentials { path });
            }
            Err(source) => {
                return Err(LocalError::Io {
                    what: "realm credential file",
                    source,
                });
            }
        };
        let stored: StoredCredentials =
            serde_json::from_slice(&bytes).map_err(|_| LocalError::Malformed {
                what: "realm credential file",
            })?;
        if stored.schema_version != LOCAL_SCHEMA {
            return Err(LocalError::Malformed {
                what: "realm credential file",
            });
        }
        let secret = match tier {
            CallerTier::Observer => stored.observer,
            CallerTier::Operator => stored.operator,
            CallerTier::Admin => stored.admin,
        };
        Ok(Self {
            tier,
            secret: SecretString::from(secret),
        })
    }

    /// The tier this credential carries.
    #[must_use]
    pub const fn tier(&self) -> CallerTier {
        self.tier
    }
}

/// Where this caller's Realm is listening.
///
/// Resolution order is explicit flag, then the Realm's own `endpoint.json`, then
/// the default loopback port — and the answer is validated as loopback whichever
/// of the three produced it.
#[derive(Debug, Clone)]
pub struct Endpoint {
    base_url: url::Url,
    authority: String,
}

impl Endpoint {
    /// Resolve where to call.
    ///
    /// # Errors
    /// Returns [`LocalError::NotAUrl`] for an unparseable base URL,
    /// [`LocalError::NotLoopback`] for one that names anything but this machine,
    /// and [`LocalError::Malformed`] when the Realm's endpoint file is not a
    /// document this build wrote.
    pub fn resolve(state_root: &Path, explicit: Option<&str>) -> Result<Self, LocalError> {
        if let Some(base_url) = explicit {
            return Self::parse(base_url);
        }
        let path = state_root.join(ENDPOINT_FILE);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let stored: StoredEndpoint =
                    serde_json::from_slice(&bytes).map_err(|_| LocalError::Malformed {
                        what: "realm endpoint file",
                    })?;
                if stored.schema_version != LOCAL_SCHEMA {
                    return Err(LocalError::Malformed {
                        what: "realm endpoint file",
                    });
                }
                Self::parse(&stored.base_url)
            }
            // No endpoint file is the ordinary case for a realm serving the
            // default port, so it is a default rather than a failure. A realm on
            // another port writes the file at startup, and a caller pointed at a
            // realm that is not running gets a transport failure — which is the
            // honest report, because nothing local is wrong.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Self::parse(&format!("http://127.0.0.1:{DEFAULT_PORT}"))
            }
            Err(source) => Err(LocalError::Io {
                what: "realm endpoint file",
                source,
            }),
        }
    }

    /// Parse and validate one base URL.
    ///
    /// # Errors
    /// As [`Endpoint::resolve`].
    pub fn parse(base_url: &str) -> Result<Self, LocalError> {
        let trimmed = base_url.trim_end_matches('/');
        let parsed = url::Url::parse(trimmed).map_err(|_| LocalError::NotAUrl {
            base_url: base_url.to_owned(),
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(LocalError::NotAUrl {
                base_url: base_url.to_owned(),
            });
        }
        let host = parsed.host_str().ok_or_else(|| LocalError::NotAUrl {
            base_url: base_url.to_owned(),
        })?;
        if !is_loopback_host(host) {
            return Err(LocalError::NotLoopback {
                base_url: base_url.to_owned(),
            });
        }
        // The authority is what goes in the `Host` header, and it is rebuilt from
        // the parts the URL parser actually understood rather than sliced out of
        // the input — the same reason the daemon's ingress check rebuilds it.
        let authority = match parsed.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        Ok(Self {
            base_url: parsed,
            authority,
        })
    }

    /// The authority to present as `Host`.
    #[must_use]
    pub fn authority(&self) -> &str {
        &self.authority
    }

    /// The base URL.
    #[must_use]
    pub const fn base_url(&self) -> &url::Url {
        &self.base_url
    }
}

/// Whether a host names this machine.
///
/// Only the loopback spellings are accepted. A hostname that happens to resolve
/// to `127.0.0.1` today is the DNS-rebinding case, so it is refused however it
/// resolves — and the daemon would refuse it too, because it checks the `Host` it
/// receives. Checking here as well means a misconfigured client fails on this
/// machine with a message about configuration, rather than on the wire with a
/// forbidden it would have to interpret.
#[must_use]
pub fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let literal = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    literal
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

/// The two methods this client can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// A read.
    Get,
    /// A write, or a command intent.
    Post,
}

impl Method {
    /// The stable spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// One request against one `/v1` route.
///
/// There is no field for a base URL, a token or a tier: the transport holds all
/// three, which is what stops a tool from choosing any of them.
#[derive(Debug, Clone)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The path, always beginning `/v1/`.
    pub path: String,
    /// Query parameters, in the order they should appear.
    pub query: Vec<(String, String)>,
    /// The `Idempotency-Key` this mutation is committed under.
    pub idempotency_key: Option<String>,
    /// The JSON body, for a `POST`.
    pub body: Option<serde_json::Value>,
}

impl Request {
    /// A `GET` of one route.
    #[must_use]
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            path: path.into(),
            query: Vec::new(),
            idempotency_key: None,
            body: None,
        }
    }

    /// A `POST` of one document.
    #[must_use]
    pub fn post(path: impl Into<String>, body: serde_json::Value) -> Self {
        Self {
            method: Method::Post,
            path: path.into(),
            query: Vec::new(),
            idempotency_key: None,
            body: Some(body),
        }
    }

    /// Add one query parameter.
    #[must_use]
    pub fn with_query(mut self, name: &str, value: impl fmt::Display) -> Self {
        self.query.push((name.to_owned(), value.to_string()));
        self
    }

    /// Add one query parameter when it is present.
    #[must_use]
    pub fn with_optional_query(self, name: &str, value: Option<impl fmt::Display>) -> Self {
        match value {
            None => self,
            Some(value) => self.with_query(name, value),
        }
    }

    /// Commit this mutation under a stable key.
    #[must_use]
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }
}

/// One whole answer from the daemon.
#[derive(Debug, Clone)]
pub struct Reply {
    /// The HTTP status.
    pub status: u16,
    /// The JSON body.
    pub body: serde_json::Value,
}

/// The daemon's own refusal, relayed rather than interpreted.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}")]
pub struct Refusal {
    /// The status it was reported with.
    pub status: u16,
    /// The stable machine code, read out of the body and not rewritten.
    pub code: String,
    /// The `ApiErrorBody` exactly as the daemon sent it.
    pub body: serde_json::Value,
}

/// The absence of an answer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TransportFailure {
    /// The daemon could not be reached at all.
    #[error("the realm could not be reached at {base_url}: is `kontor-daemon` running?")]
    Unreachable {
        /// Where the call was attempted.
        base_url: String,
    },
    /// Something answered, but not with the contract.
    ///
    /// A body that is not JSON, a refusal with no `code`, or an event stream whose
    /// frames are not JSON. Every one of them means the thing on the other end is
    /// not a Kontor Realm of this generation, which is a different problem from
    /// being refused by one.
    #[error("the answer from {path} was not this contract: {detail}")]
    Protocol {
        /// The route that answered.
        path: String,
        /// What was wrong with it. Never the body itself.
        detail: &'static str,
    },
}

/// Everything one call can end as.
#[derive(Debug, thiserror::Error)]
pub enum CallFailure {
    /// Nothing was sent, because this machine is misconfigured.
    #[error(transparent)]
    Local(#[from] LocalError),
    /// The Realm said no.
    #[error(transparent)]
    Refused(#[from] Refusal),
    /// There was no answer.
    #[error(transparent)]
    Transport(#[from] TransportFailure),
}

/// How much of an event stream one read is willing to take.
///
/// Both bounds exist because an event stream has no end: `/v1/events` waits when
/// it is caught up, so a reader with no budget would never return. Naming a
/// budget is what lets a command emit one JSON value and exit.
#[derive(Debug, Clone, Copy)]
pub struct FrameBudget {
    /// Stop after this many frames.
    pub max_frames: usize,
    /// Stop when no frame has arrived for this long.
    pub idle: Duration,
}

impl Default for FrameBudget {
    fn default() -> Self {
        Self {
            max_frames: 100,
            idle: Duration::from_millis(2_000),
        }
    }
}

/// One frame of an event stream.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    /// The SSE event name — `control`, `content` or `error`.
    pub event: String,
    /// The SSE id: a control-plane cursor, or an `epoch:sequence` content
    /// position. Relayed as text, so the two spaces cannot be added together.
    pub id: String,
    /// The frame's JSON payload.
    pub data: serde_json::Value,
}

/// The narrow thing a caller is allowed to do to a Realm.
///
/// It is a trait so a test can drive the real command and tool layers against a
/// recording fake and then assert what was *not* dispatched. Proving that an
/// observer's mutation attempt never reached the daemon needs a transport that
/// can be asked "what did you receive"; a mock HTTP server could answer that too,
/// but it would have to bind a socket, and no test in this workspace does that.
#[async_trait::async_trait]
pub trait Transport: Send + Sync + fmt::Debug {
    /// Which tier this transport carries. Fixed when it was built.
    fn tier(&self) -> CallerTier;

    /// Where it calls. For error messages only.
    fn base_url(&self) -> String;

    /// Make one request and read the whole answer.
    ///
    /// # Errors
    /// Returns [`TransportFailure`] when there is no answer or the answer is not
    /// this contract. A refusal is *not* an error here: it comes back as a
    /// [`Reply`] with its status, and the caller decides what a non-2xx means.
    async fn call(&self, request: &Request) -> Result<Reply, TransportFailure>;

    /// Read a bounded prefix of one event stream.
    ///
    /// On success the reply's body is `{"frames": [...]}`, so a stream and a
    /// document travel the same path and are refused the same way.
    ///
    /// # Errors
    /// As [`Transport::call`].
    async fn frames(
        &self,
        request: &Request,
        budget: FrameBudget,
    ) -> Result<Reply, TransportFailure>;
}

/// The real loopback transport.
pub struct HttpTransport {
    client: reqwest::Client,
    endpoint: Endpoint,
    credential: Credential,
}

impl fmt::Debug for HttpTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpTransport")
            .field("base_url", &self.endpoint.base_url().as_str())
            .field("tier", &self.credential.tier())
            .finish_non_exhaustive()
    }
}

impl HttpTransport {
    /// Build a transport for one Realm at one tier.
    ///
    /// # Errors
    /// Returns [`LocalError::Client`] when the HTTP client cannot be built.
    pub fn new(endpoint: Endpoint, credential: Credential) -> Result<Self, LocalError> {
        let client = reqwest::Client::builder()
            // A loopback control plane has no business following a redirect: the
            // only place a redirect could send this client is somewhere that is
            // not the realm it authenticated to.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| LocalError::Client)?;
        Ok(Self {
            client,
            endpoint,
            credential,
        })
    }

    /// Build the URL and the headers for one request.
    fn prepare(&self, request: &Request) -> Result<reqwest::RequestBuilder, TransportFailure> {
        let mut url = self.endpoint.base_url().join(&request.path).map_err(|_| {
            TransportFailure::Protocol {
                path: request.path.clone(),
                detail: "the route is not a path this client can address",
            }
        })?;
        if !request.query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in &request.query {
                pairs.append_pair(name, value);
            }
        }
        let mut builder = match request.method {
            Method::Get => self.client.get(url),
            Method::Post => self.client.post(url),
        };
        builder = builder
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.credential.secret.expose_secret()),
            )
            // Set explicitly rather than left to the client. The daemon admits a
            // request on the `Host` it receives, so the value that decides
            // admission is one this client chose.
            .header(reqwest::header::HOST, self.endpoint.authority());
        if let Some(key) = &request.idempotency_key {
            builder = builder.header("idempotency-key", key);
        }
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }
        Ok(builder)
    }
}

#[async_trait::async_trait]
impl Transport for HttpTransport {
    fn tier(&self) -> CallerTier {
        self.credential.tier()
    }

    fn base_url(&self) -> String {
        self.endpoint.base_url().to_string()
    }

    async fn call(&self, request: &Request) -> Result<Reply, TransportFailure> {
        let response =
            self.prepare(request)?
                .send()
                .await
                .map_err(|_| TransportFailure::Unreachable {
                    base_url: self.base_url(),
                })?;
        let status = response.status().as_u16();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| TransportFailure::Unreachable {
                base_url: self.base_url(),
            })?;
        // An empty body with a success status is a document-free answer, which
        // this contract does not have; reporting it as a protocol failure is
        // honest, where inventing `null` would let a caller print "success".
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| TransportFailure::Protocol {
                path: request.path.clone(),
                detail: "the body was not JSON",
            })?;
        Ok(Reply { status, body })
    }

    async fn frames(
        &self,
        request: &Request,
        budget: FrameBudget,
    ) -> Result<Reply, TransportFailure> {
        let response = self
            .prepare(request)?
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|_| TransportFailure::Unreachable {
                base_url: self.base_url(),
            })?;
        let status = response.status().as_u16();
        if status != 200 {
            // A refused stream answers with the same `ApiErrorBody` every other
            // route does, so it is read as a document and relayed unchanged.
            let bytes = response
                .bytes()
                .await
                .map_err(|_| TransportFailure::Unreachable {
                    base_url: self.base_url(),
                })?;
            let body: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| TransportFailure::Protocol {
                    path: request.path.clone(),
                    detail: "the refusal body was not JSON",
                })?;
            return Ok(Reply { status, body });
        }

        let mut stream = response.bytes_stream();
        let mut pending = String::new();
        let mut frames = Vec::new();
        while frames.len() < budget.max_frames {
            let next = tokio::time::timeout(budget.idle, stream.next()).await;
            let chunk = match next {
                // The idle bound elapsed: the realm is caught up and this read is
                // done. Not a failure — a stream that waits is the stream working.
                Err(_) => break,
                Ok(None) => break,
                Ok(Some(Err(_))) => {
                    return Err(TransportFailure::Unreachable {
                        base_url: self.base_url(),
                    });
                }
                Ok(Some(Ok(chunk))) => chunk,
            };
            pending.push_str(&String::from_utf8_lossy(&chunk));
            // A block is complete only at a blank line, so a frame split across
            // two chunks is assembled rather than delivered in halves.
            while let Some(split) = pending.find("\n\n") {
                let block: String = pending.drain(..split + 2).collect();
                if let Some(frame) = parse_frame(&block, &request.path)? {
                    frames.push(frame);
                }
                if frames.len() >= budget.max_frames {
                    break;
                }
            }
        }
        Ok(Reply {
            status,
            body: serde_json::json!({ "frames": frames }),
        })
    }
}

/// Parse one SSE block, or `None` when it carries no data.
///
/// Keep-alive comments and the framing of a stream that has not produced anything
/// yet both arrive as data-free blocks, and neither is an event.
fn parse_frame(block: &str, path: &str) -> Result<Option<Frame>, TransportFailure> {
    let mut event = String::new();
    let mut id = String::new();
    let mut data = String::new();
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            event = rest.trim().to_owned();
        } else if let Some(rest) = line.strip_prefix("id:") {
            id = rest.trim().to_owned();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data.push_str(rest.trim());
        }
    }
    if data.is_empty() {
        return Ok(None);
    }
    let parsed = serde_json::from_str(&data).map_err(|_| TransportFailure::Protocol {
        path: path.to_owned(),
        detail: "an event frame did not carry JSON",
    })?;
    Ok(Some(Frame {
        event,
        id,
        data: parsed,
    }))
}

/// A transport, plus the Realm it is expected to be talking to.
///
/// The expectation is established once, from `GET /v1/realm`, and then checked
/// against every later answer. That check is the reason this type exists: a
/// caller holding a cached identifier from one Realm and pointing at another must
/// be told so, and it must be told in the vocabulary the contract already has —
/// `realm_mismatch` — rather than by quietly showing it another Realm's rows.
pub struct RealmClient {
    transport: Box<dyn Transport>,
    expected: Mutex<Option<String>>,
}

impl fmt::Debug for RealmClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealmClient")
            .field("transport", &self.transport)
            .finish_non_exhaustive()
    }
}

impl RealmClient {
    /// Wrap a transport with no expectation yet.
    #[must_use]
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            expected: Mutex::new(None),
        }
    }

    /// Wrap a transport that is already expected to answer for `realm_id`.
    #[must_use]
    pub fn expecting(transport: Box<dyn Transport>, realm_id: String) -> Self {
        Self {
            transport,
            expected: Mutex::new(Some(realm_id)),
        }
    }

    /// The tier every call is made at.
    #[must_use]
    pub fn tier(&self) -> CallerTier {
        self.transport.tier()
    }

    /// Where this client calls.
    #[must_use]
    pub fn base_url(&self) -> String {
        self.transport.base_url()
    }

    /// The Realm this client expects, once it has been established.
    #[must_use]
    pub fn expected_realm(&self) -> Option<String> {
        self.expected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Establish which Realm this endpoint is, by asking it.
    ///
    /// # Errors
    /// As [`RealmClient::send`]. A realm that will not identify itself to this
    /// credential is refused rather than assumed.
    pub async fn establish_realm(&self) -> Result<String, CallFailure> {
        if let Some(known) = self.expected_realm() {
            return Ok(known);
        }
        // The send records the expectation on the way through, so an absent
        // expectation afterwards means the body named no realm at all.
        self.send(&Request::get("/v1/realm")).await?;
        self.expected_realm()
            .ok_or(CallFailure::Transport(TransportFailure::Protocol {
                path: "/v1/realm".to_owned(),
                detail: "the realm route did not name a realm",
            }))
    }

    /// Make one call, and hold the answer to the Realm expectation.
    ///
    /// # Errors
    /// Returns [`CallFailure::Refused`] for any non-2xx answer, relaying the
    /// daemon's own body; [`CallFailure::Transport`] when there was no answer;
    /// and a synthesized `realm_mismatch` refusal when the answer names a Realm
    /// this client is not talking to.
    pub async fn send(&self, request: &Request) -> Result<Reply, CallFailure> {
        let reply = self.transport.call(request).await?;
        self.admit(request, reply)
    }

    /// Read a bounded prefix of one event stream, under the same rules.
    ///
    /// # Errors
    /// As [`RealmClient::send`].
    pub async fn stream(
        &self,
        request: &Request,
        budget: FrameBudget,
    ) -> Result<Reply, CallFailure> {
        let reply = self.transport.frames(request, budget).await?;
        self.admit(request, reply)
    }

    /// Turn one answer into a reply or a refusal, checking the Realm either way.
    fn admit(&self, request: &Request, reply: Reply) -> Result<Reply, CallFailure> {
        let named = reply
            .body
            .get("realm_id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        if let Some(named) = named {
            let mut expected = self
                .expected
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match expected.as_ref() {
                None => *expected = Some(named),
                Some(known) if *known == named => {}
                Some(known) => {
                    // A local mismatch, reported in the contract's own vocabulary
                    // and shaped exactly like the daemon's own refusal — so a
                    // caller branching on `code` does not have to know which side
                    // noticed. The realm named is the one this client belongs to.
                    return Err(CallFailure::Refused(Refusal {
                        status: 409,
                        code: "realm_mismatch".to_owned(),
                        body: serde_json::json!({
                            "realm_id": known,
                            "code": "realm_mismatch",
                            "rule": "the endpoint answered for a different realm than this client established",
                            "current_revision": serde_json::Value::Null,
                            "oldest_retained_cursor": serde_json::Value::Null,
                            "newest_cursor": serde_json::Value::Null,
                        }),
                    }));
                }
            }
        }
        if (200..300).contains(&reply.status) {
            return Ok(reply);
        }
        let code = reply
            .body
            .get("code")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                CallFailure::Transport(TransportFailure::Protocol {
                    path: request.path.clone(),
                    detail: "a refusal carried no stable code",
                })
            })?
            .to_owned();
        Err(CallFailure::Refused(Refusal {
            status: reply.status,
            code,
            body: reply.body,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_ordered_by_policy_and_not_by_declaration() {
        assert!(CallerTier::Admin.at_least(CallerTier::Operator));
        assert!(CallerTier::Operator.at_least(CallerTier::Observer));
        assert!(!CallerTier::Observer.at_least(CallerTier::Operator));
        assert!(!CallerTier::Operator.at_least(CallerTier::Admin));
    }

    #[test]
    fn a_tier_is_named_or_refused_and_never_defaulted() {
        assert_eq!(
            CallerTier::parse("observer").expect("the observer tier"),
            CallerTier::Observer
        );
        assert_eq!(
            CallerTier::parse("admin").expect("the admin tier"),
            CallerTier::Admin
        );
        assert!(
            CallerTier::parse("Observer").is_err(),
            "the spelling is exact"
        );
        assert!(CallerTier::parse("root").is_err());
        assert!(CallerTier::parse("").is_err());
    }

    #[test]
    fn only_a_loopback_base_url_is_accepted() {
        for base in [
            "http://127.0.0.1:7717",
            "http://localhost:7717",
            "http://LocalHost:7717",
            "http://[::1]:7717",
            "http://127.0.0.2:9000",
            // A trailing slash is a spelling, not a different address.
            "http://127.0.0.1:7717/",
        ] {
            Endpoint::parse(base).unwrap_or_else(|_| panic!("{base} is a loopback endpoint"));
        }
        for base in [
            "http://kontor.example.com:7717",
            "http://10.0.0.4:7717",
            // Not loopback: the wildcards are *every* interface.
            "http://0.0.0.0:7717",
            "http://[::]:7717",
            // A name that resolves wherever its owner likes.
            "http://127.0.0.1.evil.com",
            "http://localhost.evil.com",
        ] {
            assert!(
                matches!(Endpoint::parse(base), Err(LocalError::NotLoopback { .. })),
                "{base} must be refused as non-loopback"
            );
        }
        for base in ["not a url", "ftp://127.0.0.1", "file:///tmp/kontor", ""] {
            assert!(
                matches!(Endpoint::parse(base), Err(LocalError::NotAUrl { .. })),
                "{base} must be refused as unaddressable"
            );
        }
    }

    #[test]
    fn the_host_header_carries_the_authority_the_url_named() {
        let endpoint = Endpoint::parse("http://127.0.0.1:7717").expect("a loopback endpoint");
        assert_eq!(endpoint.authority(), "127.0.0.1:7717");
        let bracketed = Endpoint::parse("http://[::1]:7717").expect("a loopback endpoint");
        assert_eq!(
            bracketed.authority(),
            "[::1]:7717",
            "an IPv6 authority keeps its brackets, which is what a Host header wants"
        );
    }

    #[test]
    fn a_credential_file_this_build_did_not_write_is_refused() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        assert!(
            matches!(
                Credential::read(directory.path(), CallerTier::Observer),
                Err(LocalError::NoCredentials { .. })
            ),
            "a state root with no credential file is named as such"
        );

        let path = directory.path().join(CREDENTIAL_FILE);
        std::fs::write(&path, b"{\"schema_version\":99}").expect("the fixture is written");
        assert!(
            matches!(
                Credential::read(directory.path(), CallerTier::Observer),
                Err(LocalError::Malformed { .. })
            ),
            "a later generation is refused rather than misread"
        );

        std::fs::write(
            &path,
            br#"{"schema_version":1,"observer":"o","operator":"p","admin":"a"}"#,
        )
        .expect("the fixture is written");
        let credential =
            Credential::read(directory.path(), CallerTier::Admin).expect("the admin secret");
        assert_eq!(credential.tier(), CallerTier::Admin);
        let printed = format!("{credential:?}");
        assert!(
            !printed.contains('a') || !printed.contains("secret"),
            "a debug rendering names the tier and never the value"
        );
    }

    #[test]
    fn a_missing_endpoint_file_is_the_default_port_and_not_a_failure() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        let endpoint = Endpoint::resolve(directory.path(), None).expect("the default endpoint");
        assert_eq!(endpoint.authority(), format!("127.0.0.1:{DEFAULT_PORT}"));

        std::fs::write(
            directory.path().join(ENDPOINT_FILE),
            br#"{"schema_version":1,"base_url":"http://127.0.0.1:9312"}"#,
        )
        .expect("the fixture is written");
        let recorded = Endpoint::resolve(directory.path(), None).expect("the recorded endpoint");
        assert_eq!(
            recorded.authority(),
            "127.0.0.1:9312",
            "a realm on another port is discovered rather than guessed"
        );

        let explicit = Endpoint::resolve(directory.path(), Some("http://localhost:1234"))
            .expect("the explicit endpoint");
        assert_eq!(
            explicit.authority(),
            "localhost:1234",
            "an explicit base url wins over the recorded one"
        );
    }

    #[test]
    fn a_data_free_sse_block_is_not_an_event() {
        assert!(
            parse_frame(": keep-alive\n\n", "/v1/events")
                .expect("a comment parses")
                .is_none(),
            "a keep-alive comment is framing and not an event"
        );
        let frame = parse_frame("event: control\nid: 7\ndata: {\"a\":1}\n\n", "/v1/events")
            .expect("a frame parses")
            .expect("the block carries data");
        assert_eq!(frame.event, "control");
        assert_eq!(
            frame.id, "7",
            "the id is relayed as text, never as a number"
        );
        assert_eq!(frame.data, serde_json::json!({"a": 1}));
    }
}
