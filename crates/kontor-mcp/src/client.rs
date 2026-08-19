//! The one loopback client: where a Realm is, which secret this caller holds, and
//! how exactly one `/v1` request is made.
//!
//! # What this module deliberately does not do
//!
//! It does not interpret an answer. [`Transport::call`] returns the daemon's
//! status and body as they arrived, and a non-2xx is a [`Reply`] like any other
//! rather than an error this crate invented. A client that renamed
//! `revision_conflict`, wrapped a receipt or synthesized a refusal the Realm never
//! issued would be a second contract with its own drift — and the receipt a caller
//! is owed lives in that body.
//!
//! It also does not choose a base URL, a credential or a tier per call. All three
//! are fixed when the transport is built, which is what stops a tool argument from
//! selecting any of them.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt as _;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};

/// The file a Realm's credential set lives in, inside its state root.
///
/// The daemon writes it; this crate only ever reads it. The name is repeated here
/// rather than imported because importing it would mean depending on
/// `kontor-daemon`, which links every runtime adapter and the store — exactly what
/// this crate must not reach. `the_local_document_names_match_the_daemons` in the
/// contract crate is what keeps the two spellings honest.
pub const CREDENTIAL_FILE: &str = "credentials.json";

/// Seat-scoped credential inherited by a native consultation process.
pub const CONSULTATION_AUTH_ENV: &str = "KONTOR_AUTH";

/// The file a Realm's loopback endpoint is recorded in, inside its state root.
///
/// Optional: a Realm serving [`DEFAULT_PORT`] does not need one. It is read when
/// present so a Realm on another port can be reached without every caller being
/// told the port by hand.
pub const ENDPOINT_FILE: &str = "endpoint.json";

/// The document generation this build is willing to read.
pub const LOCAL_SCHEMA: u32 = 1;

/// The loopback port a Kontor daemon binds when it is not told otherwise.
///
/// It mirrors `kontor_daemon::DEFAULT_PORT`, which this crate may not link, and
/// `the_default_port_matches_the_daemons` in the contract crate compares them.
pub const DEFAULT_PORT: u16 = 7717;

/// How much of the control plane one caller may reach.
///
/// The three names are the three keys of the credential file and the three tiers
/// of `kontor_api::auth::CallerCapability`. The enum is declared here rather than
/// imported for the reason [`CREDENTIAL_FILE`] is: `kontor-api` reaches SQLite.
///
/// The ordering is [`CallerTier::rank`] and nothing else, so reordering the
/// variants cannot silently promote a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerTier {
    /// Read liveness, identity, snapshots, persisted events and session content.
    Observer,
    /// Everything an observer may do, plus control-plane writes, session messages
    /// and permission responses.
    Operator,
    /// Everything an operator may do, plus credential, account and
    /// policy-authority routes.
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
    /// The HTTP client itself could not be built.
    #[error("the loopback http client could not be built")]
    Client,
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
    /// The credential- and policy-authority tier's secret.
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

/// Which of a Realm's secrets this caller holds.
///
/// Building one is the only place a secret is read off disk, and the secret does
/// not leave this struct except as an `Authorization` header value. It is never an
/// argument, never a tool input and never part of an answer.
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
    /// Exactly the configured tier is selected: a server told `observer` cannot
    /// reach the operator secret sitting in the same file.
    ///
    /// # Errors
    /// Returns [`LocalError::NoCredentials`] when the state root holds no
    /// credential file, [`LocalError::Io`] when it cannot be read, and
    /// [`LocalError::Malformed`] when it is not a credential set this build wrote.
    pub fn read(state_root: &Path, tier: CallerTier) -> Result<Self, LocalError> {
        if tier == CallerTier::Operator
            && let Ok(secret) = std::env::var(CONSULTATION_AUTH_ENV)
            && secret.starts_with("kontor-seat-v1.")
        {
            return Ok(Self {
                tier,
                secret: SecretString::from(secret),
            });
        }
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

    /// Build one from an already-held secret, for a test harness.
    #[must_use]
    pub const fn from_secret(tier: CallerTier, secret: SecretString) -> Self {
        Self { tier, secret }
    }

    /// The tier this credential carries.
    #[must_use]
    pub const fn tier(&self) -> CallerTier {
        self.tier
    }
}

/// Where this caller's Realm is listening.
///
/// Resolution order is the explicit flag, then the Realm's own [`ENDPOINT_FILE`],
/// then [`DEFAULT_PORT`] — and the answer is validated as loopback whichever of the
/// three produced it. There is no way to reach a non-loopback address: not through
/// configuration, and not through a tool argument, because a tool has no argument
/// that reaches here.
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
            // default port, so it is a default rather than a failure. A caller
            // pointed at a realm that is not running gets a transport failure —
            // the honest report, because nothing local is wrong.
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
        // The authority is what goes in the `Host` header, rebuilt from the parts
        // the URL parser actually understood rather than sliced out of the input —
        // the same reason the daemon's ingress check rebuilds it.
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
/// Only the loopback spellings are accepted. A hostname that happens to resolve to
/// `127.0.0.1` today is the DNS-rebinding case, so it is refused however it
/// resolves. Checking here as well as in the daemon means a misconfigured client
/// fails on this machine with a message about configuration, rather than on the
/// wire with a refusal it would have to interpret.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Method {
    /// A read.
    Get,
    /// A write, a command intent, or a computed plan.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The path, always beginning `/v1/`.
    pub path: String,
    /// Query parameters, in the order they should appear.
    pub query: Vec<(String, String)>,
    /// The `Idempotency-Key` this mutation is committed under.
    ///
    /// Supplied by the caller and mapped only to the header. This crate never
    /// generates one: a key it invented would make a retry look like a new
    /// intent to the one component that decides what a retry means.
    pub idempotency_key: Option<String>,
    /// The JSON body, for a `POST`.
    pub body: Option<serde_json::Value>,
}

/// One whole answer from the daemon, relayed unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The HTTP status, exactly as it arrived.
    pub status: u16,
    /// The JSON body, exactly as it arrived.
    pub body: serde_json::Value,
}

impl Reply {
    /// Whether the daemon answered with a success status.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// The daemon's own stable machine code, when it sent one.
    ///
    /// Read, never rewritten: the CLI picks an exit class from it and this crate
    /// relays the body whole either way.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.body.get("code").and_then(serde_json::Value::as_str)
    }
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
    /// A body that is not JSON, or an event stream whose frames are not JSON.
    /// Either means the thing on the other end is not a Kontor Realm of this
    /// generation, which is a different problem from being refused by one.
    ///
    /// The status travels because it is the one fact that separates the two
    /// realistic causes, and reading it costs nothing. A `404` means something
    /// that is not this daemon is on the port; a `4xx` on a route that exists
    /// means a daemon *older than the typed body extractor*, still answering its
    /// own rejections with `text/plain`. Without the status those are one
    /// indistinguishable "not this contract", and the second one — an operator
    /// running a stale binary — reads as a dead realm.
    #[error("the answer from {path} was not this contract{}: {detail}", status.map_or_else(String::new, |status| format!(" (HTTP {status})")))]
    Protocol {
        /// The route that answered.
        path: String,
        /// The status it answered with, when there was a response at all.
        status: Option<u16>,
        /// What was wrong with it. Never the body itself.
        detail: &'static str,
    },
}

/// How much of an event stream one read is willing to take.
///
/// Both bounds exist because an event stream has no end: `/v1/events` waits when it
/// is caught up, so a reader with no budget would never return. Naming a budget is
/// what lets one tool call answer with one value and stop — a continuation is
/// another explicit call, never a reconnect this crate performed on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Frame {
    /// The SSE event name — `control`, `content` or `error`.
    pub event: String,
    /// The SSE id: a control-plane cursor, or an `epoch:sequence` content position.
    /// Relayed as text, so the two spaces cannot be added together.
    pub id: String,
    /// The frame's JSON payload.
    pub data: serde_json::Value,
}

/// The narrow thing a caller is allowed to do to a Realm.
///
/// It is a trait so a test can drive the real dispatch path against a recording
/// fake and then assert what was *not* sent. Proving that an observer's mutation
/// never reached the daemon needs a transport that can be asked "what did you
/// receive"; counting requests is only meaningful because the refusal happens
/// before this trait is called at all.
#[async_trait::async_trait]
pub trait Transport: Send + Sync + fmt::Debug {
    /// Which tier this transport carries. Fixed when it was built.
    fn tier(&self) -> CallerTier;

    /// Where it calls. For error messages only.
    fn base_url(&self) -> String;

    /// Make exactly one request and read the whole answer.
    ///
    /// # Errors
    /// Returns [`TransportFailure`] when there is no answer or the answer is not
    /// this contract. A refusal is *not* an error: it comes back as a [`Reply`]
    /// carrying the daemon's own status and body.
    async fn call(&self, request: &Request) -> Result<Reply, TransportFailure>;

    /// Read a bounded prefix of one event stream, from one response.
    ///
    /// # Errors
    /// As [`Transport::call`].
    async fn frames(
        &self,
        request: &Request,
        budget: FrameBudget,
    ) -> Result<Reply, TransportFailure>;
}

/// A shared transport is still a transport.
///
/// This exists for the capability tests: they hand the dispatcher a transport and
/// then need to ask that same transport what it received. Without this, proving
/// "zero requests were made" would need the dispatcher to expose its transport,
/// which is a hole in the wrong wall.
#[async_trait::async_trait]
impl<T: Transport + ?Sized> Transport for std::sync::Arc<T> {
    fn tier(&self) -> CallerTier {
        (**self).tier()
    }

    fn base_url(&self) -> String {
        (**self).base_url()
    }

    async fn call(&self, request: &Request) -> Result<Reply, TransportFailure> {
        (**self).call(request).await
    }

    async fn frames(
        &self,
        request: &Request,
        budget: FrameBudget,
    ) -> Result<Reply, TransportFailure> {
        (**self).frames(request, budget).await
    }
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
            // not the realm it authenticated to — with the bearer attached.
            .redirect(reqwest::redirect::Policy::none())
            // Environment proxies are ignored for the same reason: a loopback call
            // that went through a proxy would carry the credential off-machine.
            .no_proxy()
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
                status: None,
                detail: "the route is not a path this client can address",
            }
        })?;
        // A joined path that left loopback is not addressable by this client. It
        // cannot happen through a tool argument — paths are built from the
        // registry's own templates — and it is checked anyway, because the check
        // costs nothing and the failure it prevents is a credential leaving.
        if !url.host_str().is_some_and(is_loopback_host) {
            return Err(TransportFailure::Protocol {
                path: request.path.clone(),
                status: None,
                detail: "the route resolved off loopback",
            });
        }
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
        // An empty body with a success status is a document-free answer, which this
        // contract does not have; reporting it as a protocol failure is honest,
        // where inventing `null` would let a caller print "success".
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| TransportFailure::Protocol {
                path: request.path.clone(),
                status: Some(status),
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
            // A refused stream answers with the same error body every other route
            // does, so it is read as a document and relayed unchanged.
            let bytes = response
                .bytes()
                .await
                .map_err(|_| TransportFailure::Unreachable {
                    base_url: self.base_url(),
                })?;
            let body: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| TransportFailure::Protocol {
                    path: request.path.clone(),
                    status: Some(status),
                    detail: "the refusal body was not JSON",
                })?;
            return Ok(Reply { status, body });
        }

        // Every frame below comes out of this one response. There is no reconnect
        // and no second request: when the budget is spent the read stops, and a
        // continuation is another explicit tool call with the cursor the caller
        // read off these frames.
        let mut stream = response.bytes_stream();
        let mut pending = String::new();
        let mut frames = Vec::new();
        while frames.len() < budget.max_frames {
            let next = tokio::time::timeout(budget.idle, stream.next()).await;
            let chunk = match next {
                // The idle bound elapsed: the realm is caught up and this read is
                // done. Not a failure — a stream that waits is the stream working.
                Err(_) | Ok(None) => break,
                Ok(Some(Err(_))) => {
                    return Err(TransportFailure::Unreachable {
                        base_url: self.base_url(),
                    });
                }
                Ok(Some(Ok(chunk))) => chunk,
            };
            pending.push_str(&String::from_utf8_lossy(&chunk));
            // A block is complete only at a blank line, so a frame split across two
            // chunks is assembled rather than delivered in halves.
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
    // A frame arrives inside an already-accepted `200` stream, so there is no
    // per-frame status to report.
    let parsed = serde_json::from_str(&data).map_err(|_| TransportFailure::Protocol {
        path: path.to_owned(),
        status: None,
        detail: "an event frame did not carry JSON",
    })?;
    Ok(Some(Frame {
        event,
        id,
        data: parsed,
    }))
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
            // The URL parser normalizes a decimal or hexadecimal IPv4 literal to
            // its canonical form *before* this check sees it, so what reaches the
            // wire as `Host` is `127.0.0.1` and the daemon admits it on that
            // spelling. Accepted because the address really is loopback, not
            // because the grammar was not looked at.
            "http://2130706433",
            "http://0x7f.0.0.1",
        ] {
            let parsed =
                Endpoint::parse(base).unwrap_or_else(|_| panic!("{base} is a loopback endpoint"));
            assert!(
                is_loopback_host(parsed.base_url().host_str().expect("a host")),
                "{base} must present a loopback authority on the wire"
            );
        }
        for base in [
            "http://kontor.example.com:7717",
            "http://10.0.0.4:7717",
            "http://0.0.0.0:7717",
            "http://[::]:7717",
            // The rebinding shapes.
            "http://127.0.0.1.evil.com",
            "http://localhost.evil.com",
            // Not a URL, or not a scheme this client speaks.
            "file:///etc/passwd",
            "ftp://127.0.0.1",
            "not a url",
            "",
        ] {
            assert!(
                Endpoint::parse(base).is_err(),
                "{base} must not be reachable"
            );
        }
    }

    #[test]
    fn the_credential_is_absent_from_debug_output() {
        let credential =
            Credential::from_secret(CallerTier::Admin, SecretString::from("super-secret-value"));
        let rendered = format!("{credential:?}");
        assert!(
            !rendered.contains("super-secret-value"),
            "a secret must never be printable: {rendered}"
        );
        assert!(
            rendered.to_lowercase().contains("admin"),
            "the tier is safe to name: {rendered}"
        );
    }

    #[test]
    fn a_reply_reports_the_daemons_own_status_and_code_without_rewriting_them() {
        let refusal = Reply {
            status: 409,
            body: serde_json::json!({ "code": "revision_conflict", "current_revision": 7 }),
        };
        assert!(!refusal.is_success());
        assert_eq!(refusal.code(), Some("revision_conflict"));
        assert_eq!(
            refusal.body,
            serde_json::json!({ "code": "revision_conflict", "current_revision": 7 }),
            "the body is relayed whole, so the revision the caller is owed survives"
        );
    }

    #[test]
    fn an_endpoint_file_is_read_when_present_and_defaulted_when_absent() {
        let root = tempfile::tempdir().expect("a temporary state root");
        let resolved = Endpoint::resolve(root.path(), None).expect("the default endpoint");
        assert_eq!(
            resolved.base_url().as_str(),
            format!("http://127.0.0.1:{DEFAULT_PORT}/"),
            "a realm on the default port needs no endpoint file"
        );

        std::fs::write(
            root.path().join(ENDPOINT_FILE),
            serde_json::to_vec(&StoredEndpoint {
                schema_version: LOCAL_SCHEMA,
                base_url: "http://127.0.0.1:9931".to_owned(),
            })
            .expect("a document"),
        )
        .expect("the endpoint file is written");
        let resolved = Endpoint::resolve(root.path(), None).expect("the recorded endpoint");
        assert_eq!(resolved.authority(), "127.0.0.1:9931");

        // An explicit flag wins over the file, and is validated the same way.
        let resolved = Endpoint::resolve(root.path(), Some("http://localhost:8080"))
            .expect("the explicit endpoint");
        assert_eq!(resolved.authority(), "localhost:8080");
        assert!(
            Endpoint::resolve(root.path(), Some("http://evil.example")).is_err(),
            "an explicit non-loopback address is refused like any other"
        );
    }

    #[test]
    fn a_credential_file_yields_exactly_the_tier_that_was_asked_for() {
        let root = tempfile::tempdir().expect("a temporary state root");
        std::fs::write(
            root.path().join(CREDENTIAL_FILE),
            serde_json::json!({
                "schema_version": LOCAL_SCHEMA,
                "observer": "observer-secret",
                "operator": "operator-secret",
                "admin": "admin-secret",
            })
            .to_string(),
        )
        .expect("the credential file is written");

        for tier in CallerTier::ALL {
            let credential = Credential::read(root.path(), *tier).expect("the tier's secret");
            assert_eq!(credential.tier(), *tier);
            assert_eq!(
                credential.secret.expose_secret(),
                format!("{tier}-secret"),
                "a server configured for one tier reads that tier and no other"
            );
        }
    }

    #[test]
    fn a_missing_or_unreadable_credential_file_is_a_local_failure_naming_the_path() {
        let root = tempfile::tempdir().expect("a temporary state root");
        let failure = Credential::read(root.path(), CallerTier::Admin)
            .expect_err("there is no credential file");
        assert!(matches!(failure, LocalError::NoCredentials { .. }));

        std::fs::write(root.path().join(CREDENTIAL_FILE), b"not json").expect("written");
        let failure =
            Credential::read(root.path(), CallerTier::Admin).expect_err("the file is not a set");
        assert!(matches!(failure, LocalError::Malformed { .. }));
        assert!(
            !format!("{failure}").contains("not json"),
            "a parse failure must not quote a file of secrets"
        );
    }

    #[test]
    fn a_frame_is_assembled_only_at_a_block_boundary_and_data_free_blocks_are_dropped() {
        let frame = parse_frame("event: control\nid: 12\ndata: {\"a\":1}\n\n", "/v1/events")
            .expect("a well-formed block")
            .expect("it carries data");
        assert_eq!(frame.event, "control");
        assert_eq!(frame.id, "12");
        assert_eq!(frame.data, serde_json::json!({ "a": 1 }));

        // A keep-alive comment carries no data and is not an event.
        assert!(
            parse_frame(": keep-alive\n\n", "/v1/events")
                .expect("a comment block")
                .is_none()
        );
        assert!(
            parse_frame("event: content\n\n", "/v1/events")
                .expect("a data-free block")
                .is_none()
        );
        // A frame whose data is not JSON means the other end is not this contract.
        assert!(parse_frame("data: not json\n\n", "/v1/events").is_err());
    }
}
