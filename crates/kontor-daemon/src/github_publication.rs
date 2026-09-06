//! The GitHub App side of publication identity (ASMA-8101, PUB-02).
//!
//! Kontor's attestation says whether a branch, commit and title are the work
//! they claim to be. The forge only learns that if something tells it, and the
//! only identity that may tell it is one that cannot bypass the rules it
//! reports on: a GitHub App with `checks: write`, `pull_requests: read` and
//! `contents: write` for the squash merge, and nothing else. This module is that
//! App's client — JWT, installation token, check runs, pull-request reads and the
//! head-bound squash merge — plus the poller that re-judges every open pull
//! request and posts `asma/publication-identity` for it.
//!
//! Everything here is inert until an operator installs the App and writes
//! `<state root>/config/github-app.json`. Absent configuration is valid; a
//! present document that will not parse refuses the start, as the quota-signal
//! document does. The private key is read from a path the document names and
//! never from the document itself.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use kontor_api::applications::{ApplicationOperations, PublicationIdentityRequest};
use kontor_api::state::{ApiState, BarrierState};
use kontor_core::id::{ExternalName, IdempotencyKey, ProjectId, Timestamp};
use kontor_core::publication::CommitSha;
use serde::{Deserialize, Serialize};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

use crate::applications::Services;

/// Where the App configuration lives below the state root.
pub const CONFIG_FILE: &str = "config/github-app.json";
/// The check run every judged pull request carries.
pub const DEFAULT_CHECK_NAME: &str = "asma/publication-identity";
/// GitHub's REST API version this client speaks.
const API_VERSION: &str = "2022-11-28";
/// How long a request to GitHub may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// App JWTs live at most ten minutes; issue them slightly in the past so a
/// clock a few seconds ahead of GitHub's still validates.
const JWT_BACKDATE_SECS: i64 = 60;
const JWT_LIFETIME_SECS: i64 = 540;
/// Installation tokens live an hour; refresh with a margin.
const TOKEN_MARGIN: Duration = Duration::from_secs(120);

/// The operator's App configuration document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubPublicationConfig {
    /// Document generation.
    pub schema_version: u32,
    /// The App id GitHub assigned.
    pub app_id: u64,
    /// The installation of that App on the organisation.
    pub installation_id: u64,
    /// The PEM private key GitHub issued, stored outside every repository.
    pub private_key_path: PathBuf,
    /// The Kontor project whose bindings judge these repositories.
    pub project_id: ProjectId,
    /// The `owner/name` repositories the App is installed on and judges.
    pub repositories: Vec<String>,
    /// The check-run name. Defaults to [`DEFAULT_CHECK_NAME`].
    #[serde(default = "default_check_name")]
    pub check_name: String,
    /// Seconds between polls of open pull requests.
    #[serde(default = "default_poll_seconds")]
    pub poll_seconds: u64,
    /// The API base, overridable for tests and GitHub Enterprise Server.
    #[serde(default = "default_api_base")]
    pub api_base: String,
}

fn default_check_name() -> String {
    DEFAULT_CHECK_NAME.to_owned()
}

fn default_poll_seconds() -> u64 {
    60
}

fn default_api_base() -> String {
    "https://api.github.com".to_owned()
}

/// Why the App configuration or key could not be used.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The document exists but is not the expected shape.
    #[error("github-app.json is not a valid App configuration: {detail}")]
    Invalid {
        /// What was wrong, structurally; never a value from the document.
        detail: String,
    },
    /// The document could not be read.
    #[error("github-app.json could not be read")]
    Unreadable,
    /// The private key could not be read or is not an RSA private key.
    #[error("the GitHub App private key could not be loaded")]
    Key,
}

impl GithubPublicationConfig {
    /// Read the document below `state_root`, if there is one.
    ///
    /// # Errors
    /// A present document that will not parse, or names a key that will not
    /// load, is refused rather than ignored.
    pub fn read(state_root: &Path) -> Result<Option<Self>, ConfigError> {
        let path = state_root.join(CONFIG_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path).map_err(|_| ConfigError::Unreadable)?;
        let config: Self = serde_json::from_str(&text).map_err(|error| ConfigError::Invalid {
            detail: error.classify_detail(),
        })?;
        if config.schema_version != 1 {
            return Err(ConfigError::Invalid {
                detail: "schema_version must be 1".to_owned(),
            });
        }
        if config.repositories.is_empty()
            || config.repositories.iter().any(|repository| {
                repository
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .count()
                    != 2
            })
        {
            return Err(ConfigError::Invalid {
                detail: "repositories must be one or more owner/name entries".to_owned(),
            });
        }
        if config.poll_seconds == 0 {
            return Err(ConfigError::Invalid {
                detail: "poll_seconds must be at least 1".to_owned(),
            });
        }
        Ok(Some(config))
    }
}

trait ClassifyDetail {
    fn classify_detail(&self) -> String;
}

impl ClassifyDetail for serde_json::Error {
    fn classify_detail(&self) -> String {
        format!("{:?} at line {}", self.classify(), self.line())
    }
}

// ---------------------------------------------------------------------------
// Encoding helpers: small enough to own, and the alternative is a dependency.
// ---------------------------------------------------------------------------

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url without padding, as JWT segments require.
#[must_use]
pub fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let value =
            (u32::from(buffer[0]) << 16) | (u32::from(buffer[1]) << 8) | u32::from(buffer[2]);
        let symbols = [
            URL_SAFE[((value >> 18) & 63) as usize],
            URL_SAFE[((value >> 12) & 63) as usize],
            URL_SAFE[((value >> 6) & 63) as usize],
            URL_SAFE[(value & 63) as usize],
        ];
        let keep = match chunk.len() {
            1 => 2,
            2 => 3,
            _ => 4,
        };
        out.extend(symbols[..keep].iter().map(|byte| char::from(*byte)));
    }
    out
}

/// Standard base64 decoding, tolerant of whitespace and padding, as PEM bodies use.
#[must_use]
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut accumulator: u32 = 0;
    let mut bits = 0u8;
    for byte in text.bytes() {
        if byte.is_ascii_whitespace() || byte == b'=' {
            continue;
        }
        let value = STANDARD.iter().position(|symbol| *symbol == byte)? as u32;
        accumulator = (accumulator << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((accumulator >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// Load an RSA private key from PEM text, PKCS#1 (`RSA PRIVATE KEY`, what GitHub
/// issues) or PKCS#8 (`PRIVATE KEY`).
///
/// # Errors
/// [`ConfigError::Key`] for anything that is not one RSA private key.
pub fn key_pair_from_pem(pem: &str) -> Result<RsaKeyPair, ConfigError> {
    let mut body = String::new();
    let mut kind = None;
    for line in pem.lines() {
        let line = line.trim();
        if let Some(label) = line
            .strip_prefix("-----BEGIN ")
            .and_then(|rest| rest.strip_suffix("-----"))
        {
            kind = Some(label.to_owned());
            continue;
        }
        if line.starts_with("-----END ") || line.is_empty() {
            continue;
        }
        body.push_str(line);
    }
    let der = base64_decode(&body).ok_or(ConfigError::Key)?;
    match kind.as_deref() {
        Some("RSA PRIVATE KEY") => RsaKeyPair::from_der(&der).map_err(|_| ConfigError::Key),
        Some("PRIVATE KEY") => RsaKeyPair::from_pkcs8(&der).map_err(|_| ConfigError::Key),
        _ => Err(ConfigError::Key),
    }
}

/// The signed App JWT GitHub exchanges for an installation token.
///
/// # Errors
/// [`ConfigError::Key`] when the key cannot sign.
pub fn app_jwt(key: &RsaKeyPair, app_id: u64, now: Timestamp) -> Result<String, ConfigError> {
    let seconds = now.as_second();
    let header = base64url(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = base64url(
        serde_json::json!({
            "iat": seconds - JWT_BACKDATE_SECS,
            "exp": seconds + JWT_LIFETIME_SECS,
            "iss": app_id.to_string(),
        })
        .to_string()
        .as_bytes(),
    );
    let signing_input = format!("{header}.{claims}");
    let mut signature = vec![0u8; key.public_modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .map_err(|_| ConfigError::Key)?;
    Ok(format!("{signing_input}.{}", base64url(&signature)))
}

// ---------------------------------------------------------------------------
// The App client
// ---------------------------------------------------------------------------

/// One open pull request as GitHub reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PullRequestFacts {
    /// The number.
    pub number: u64,
    /// The title.
    pub title: String,
    /// The head branch and commit.
    pub head: RefFacts,
    /// The base branch.
    pub base: RefFacts,
    /// Whether it is a draft.
    #[serde(default)]
    pub draft: bool,
    /// `open`, `closed`.
    pub state: String,
    /// Whether it was merged.
    #[serde(default)]
    pub merged: bool,
}

/// One side of a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RefFacts {
    /// The branch name.
    #[serde(rename = "ref")]
    pub name: String,
    /// The commit.
    pub sha: String,
}

/// Why a call to GitHub did not produce what was asked.
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// The App is not configured on this daemon.
    #[error("no GitHub App is configured for publication checks")]
    Unconfigured,
    /// The request could not be sent or answered.
    #[error("GitHub could not be reached: {detail}")]
    Unreachable {
        /// The transport-level detail.
        detail: String,
    },
    /// GitHub answered with a refusal.
    #[error("GitHub answered {status} to {operation}")]
    Refused {
        /// The HTTP status.
        status: u16,
        /// Which call.
        operation: &'static str,
    },
    /// GitHub answered something this client cannot read.
    #[error("GitHub's answer to {operation} could not be read")]
    Unreadable {
        /// Which call.
        operation: &'static str,
    },
    /// The App key could not sign.
    #[error("the GitHub App key could not sign a token request")]
    Signing,
}

#[derive(Debug, Deserialize)]
struct InstallationToken {
    token: String,
    expires_at: String,
}

/// A GitHub App installation client bound to one configuration and key.
pub struct GithubPublicationGateway {
    config: GithubPublicationConfig,
    key: RsaKeyPair,
    client: reqwest::Client,
    token: Mutex<Option<(String, Timestamp)>>,
}

impl std::fmt::Debug for GithubPublicationGateway {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GithubPublicationGateway")
            .field("app_id", &self.config.app_id)
            .field("installation_id", &self.config.installation_id)
            .field("repositories", &self.config.repositories)
            .finish_non_exhaustive()
    }
}

impl GithubPublicationGateway {
    /// Compose the gateway from a configuration document, loading its key.
    ///
    /// # Errors
    /// [`ConfigError::Key`] when the named key cannot be read or parsed.
    pub fn from_config(config: GithubPublicationConfig) -> Result<Self, ConfigError> {
        let pem =
            std::fs::read_to_string(&config.private_key_path).map_err(|_| ConfigError::Key)?;
        let key = key_pair_from_pem(&pem)?;
        Ok(Self::with_key(config, key))
    }

    /// Compose the gateway with an already-loaded key.
    #[must_use]
    pub fn with_key(config: GithubPublicationConfig, key: RsaKeyPair) -> Self {
        Self {
            config,
            key,
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .user_agent("kontor-publication-identity")
                .build()
                .unwrap_or_default(),
            token: Mutex::new(None),
        }
    }

    /// The configuration this gateway serves.
    #[must_use]
    pub const fn config(&self) -> &GithubPublicationConfig {
        &self.config
    }

    /// Whether this gateway judges the given `owner/name` repository.
    #[must_use]
    pub fn governs(&self, repository: &str) -> bool {
        self.config
            .repositories
            .iter()
            .any(|governed| governed.eq_ignore_ascii_case(repository))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.config.api_base.trim_end_matches('/'))
    }

    /// A valid installation token, exchanged when the cached one is near expiry.
    async fn installation_token(&self) -> Result<String, GatewayError> {
        let now = kontor_api::now();
        if let Some((token, expires_at)) = self.token.lock().expect("token cache").clone()
            && expires_at.duration_since(now)
                > jiff::SignedDuration::try_from(TOKEN_MARGIN).unwrap_or_default()
        {
            return Ok(token);
        }
        let jwt = app_jwt(&self.key, self.config.app_id, now).map_err(|_| GatewayError::Signing)?;
        let response = self
            .client
            .post(self.url(&format!(
                "/app/installations/{}/access_tokens",
                self.config.installation_id
            )))
            .bearer_auth(jwt)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .send()
            .await
            .map_err(|error| GatewayError::Unreachable {
                detail: error.to_string(),
            })?;
        if !response.status().is_success() {
            return Err(GatewayError::Refused {
                status: response.status().as_u16(),
                operation: "installation token exchange",
            });
        }
        let token: InstallationToken =
            response
                .json()
                .await
                .map_err(|_| GatewayError::Unreadable {
                    operation: "installation token exchange",
                })?;
        let expires_at = token
            .expires_at
            .parse::<Timestamp>()
            .unwrap_or_else(|_| now + jiff::SignedDuration::from_secs(3000));
        *self.token.lock().expect("token cache") = Some((token.token.clone(), expires_at));
        Ok(token.token)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
        operation: &'static str,
    ) -> Result<reqwest::Response, GatewayError> {
        let token = self.installation_token().await?;
        let mut request = self
            .client
            .request(method, self.url(path))
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GatewayError::Unreachable {
                detail: error.to_string(),
            })?;
        if !response.status().is_success() {
            return Err(GatewayError::Refused {
                status: response.status().as_u16(),
                operation,
            });
        }
        Ok(response)
    }

    /// Every open pull request of one repository.
    ///
    /// # Errors
    /// [`GatewayError`] when GitHub cannot be reached or refuses.
    pub async fn open_pull_requests(
        &self,
        repository: &str,
    ) -> Result<Vec<PullRequestFacts>, GatewayError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/repos/{repository}/pulls?state=open&per_page=100"),
                None,
                "list open pull requests",
            )
            .await?;
        response.json().await.map_err(|_| GatewayError::Unreadable {
            operation: "list open pull requests",
        })
    }

    /// One pull request, fresh.
    ///
    /// # Errors
    /// [`GatewayError`] when GitHub cannot be reached or refuses.
    pub async fn pull_request(
        &self,
        repository: &str,
        number: u64,
    ) -> Result<PullRequestFacts, GatewayError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/repos/{repository}/pulls/{number}"),
                None,
                "read pull request",
            )
            .await?;
        response.json().await.map_err(|_| GatewayError::Unreadable {
            operation: "read pull request",
        })
    }

    /// Post the publication-identity check run for one head commit.
    ///
    /// # Errors
    /// [`GatewayError`] when GitHub cannot be reached or refuses.
    pub async fn post_check_run(
        &self,
        repository: &str,
        head_sha: &str,
        accepted: bool,
        summary: &str,
    ) -> Result<(), GatewayError> {
        let body = serde_json::json!({
            "name": self.config.check_name,
            "head_sha": head_sha,
            "status": "completed",
            "conclusion": if accepted { "success" } else { "failure" },
            "output": {
                "title": if accepted {
                    "Publication identity accepted"
                } else {
                    "Publication identity refused"
                },
                "summary": summary,
            },
        });
        self.request(
            reqwest::Method::POST,
            &format!("/repos/{repository}/check-runs"),
            Some(body),
            "post check run",
        )
        .await?;
        Ok(())
    }

    /// Squash-merge one pull request, only if its head is still `head_sha`.
    ///
    /// # Errors
    /// [`GatewayError::Refused`] with 405 or 409 when GitHub declines, which
    /// includes a head that moved since it was judged.
    pub async fn merge_squash(
        &self,
        repository: &str,
        number: u64,
        head_sha: &str,
    ) -> Result<String, GatewayError> {
        let response = self
            .request(
                reqwest::Method::PUT,
                &format!("/repos/{repository}/pulls/{number}/merge"),
                Some(serde_json::json!({"merge_method": "squash", "sha": head_sha})),
                "squash merge",
            )
            .await?;
        let body: serde_json::Value =
            response
                .json()
                .await
                .map_err(|_| GatewayError::Unreadable {
                    operation: "squash merge",
                })?;
        Ok(body
            .get("sha")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned())
    }
}

// ---------------------------------------------------------------------------
// Judging pull requests
// ---------------------------------------------------------------------------

/// The attestation request one pull request maps to.
///
/// # Errors
/// [`kontor_core::DomainError`] when GitHub's strings are not names Kontor keeps.
pub fn identity_of(
    repository: &str,
    facts: &PullRequestFacts,
) -> Result<PublicationIdentityRequest, kontor_core::DomainError> {
    Ok(PublicationIdentityRequest {
        repository: ExternalName::parse(repository)?,
        base_branch: ExternalName::parse(&facts.base.name)?,
        head_branch: ExternalName::parse(&facts.head.name)?,
        head_sha: facts.head.sha.clone(),
        pull_request: Some(facts.number),
        title: Some(ExternalName::parse(facts.title.trim())?),
    })
}

/// The idempotency key one (repository, pull request, head, title) is judged under.
#[must_use]
pub fn check_key(repository: &str, facts: &PullRequestFacts) -> IdempotencyKey {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(
        format!(
            "{repository}\n{}\n{}\n{}",
            facts.number, facts.head.sha, facts.title
        )
        .as_bytes(),
    );
    IdempotencyKey::parse(&format!("github-check-{}", hex::encode(&digest[..16])))
        .expect("a hex key is a valid idempotency key")
}

/// One poll: judge every open pull request of every governed repository and
/// post a check run for each head/title not yet reported this process.
pub async fn judge_open_pull_requests(
    gateway: &GithubPublicationGateway,
    services: &Services,
    posted: &mut BTreeSet<String>,
) {
    let project_id = gateway.config().project_id;
    for repository in &gateway.config().repositories {
        let pull_requests = match gateway.open_pull_requests(repository).await {
            Ok(pull_requests) => pull_requests,
            Err(error) => {
                warn!(repository, %error, "publication check could not list pull requests");
                continue;
            }
        };
        for facts in pull_requests {
            let key = check_key(repository, &facts);
            if posted.contains(key.as_str()) {
                continue;
            }
            let request = match identity_of(repository, &facts) {
                Ok(request) => request,
                Err(error) => {
                    warn!(repository, number = facts.number, %error, "pull request carries a name Kontor cannot keep");
                    continue;
                }
            };
            let decision = match services
                .attest_publication(&key, project_id, &request)
                .await
            {
                Ok(decision) => decision,
                Err(error) => {
                    warn!(repository, number = facts.number, %error, "publication could not be judged");
                    continue;
                }
            };
            let summary = if decision.accepted {
                format!(
                    "Branch `{}` at `{}` is bound to Kontor epic {} (attestation {}).",
                    request.head_branch.as_str(),
                    &facts.head.sha[..12.min(facts.head.sha.len())],
                    decision
                        .epic_id
                        .map(|id| id.to_string())
                        .unwrap_or_default(),
                    decision
                        .attestation_id
                        .map(|id| id.to_string())
                        .unwrap_or_default(),
                )
            } else {
                format!(
                    "Refused: {}. Expected `<type>/ASMA-<number>-<slug>` bound to a confirmed Kontor/Jira epic or task, base `master`, title leading with a key of that epic. Attestation {}.",
                    decision.reasons.join(", "),
                    decision
                        .attestation_id
                        .map(|id| id.to_string())
                        .unwrap_or_default(),
                )
            };
            match gateway
                .post_check_run(repository, &facts.head.sha, decision.accepted, &summary)
                .await
            {
                Ok(()) => {
                    info!(
                        repository,
                        number = facts.number,
                        accepted = decision.accepted,
                        "publication check posted"
                    );
                    posted.insert(key.as_str().to_owned());
                }
                Err(error) => {
                    warn!(repository, number = facts.number, %error, "publication check could not be posted")
                }
            }
        }
    }
}

/// Poll open pull requests until the daemon's shutdown signal is raised.
pub async fn poll_until_stopped(
    gateway: Arc<GithubPublicationGateway>,
    services: Arc<Services>,
    state: ApiState,
) {
    if state.barrier().settled().await != BarrierState::Open {
        warn!("publication checks stayed stopped because startup reconciliation failed");
        return;
    }
    let mut stops = state.signals().stops();
    let mut ticker = tokio::time::interval(Duration::from_secs(gateway.config().poll_seconds));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut posted = BTreeSet::new();
    loop {
        if state.signals().is_stopping() {
            return;
        }
        tokio::select! {
            changed = stops.changed() => {
                if changed.is_err() || *stops.borrow_and_update() {
                    return;
                }
                continue;
            }
            _ = ticker.tick() => {}
        }
        judge_open_pull_requests(&gateway, &services, &mut posted).await;
    }
}

/// A publication the merge gateway refused before touching GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeRefusal {
    /// The pull request is not open, or is a draft.
    NotOpen,
    /// Its head is not the commit the caller judged.
    HeadMoved {
        /// What GitHub reports now.
        current: String,
    },
}

/// Compare a fresh pull request read against what the caller expects.
///
/// # Errors
/// [`MergeRefusal`] naming what changed.
pub fn ensure_mergeable(
    facts: &PullRequestFacts,
    expected_head: &CommitSha,
) -> Result<(), MergeRefusal> {
    if facts.state != "open" || facts.draft || facts.merged {
        return Err(MergeRefusal::NotOpen);
    }
    if facts.head.sha != expected_head.as_str() {
        return Err(MergeRefusal::HeadMoved {
            current: facts.head.sha.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::encoding::AsDer;
    use aws_lc_rs::rsa::KeySize;
    use aws_lc_rs::signature::{KeyPair as _, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};

    fn test_key() -> RsaKeyPair {
        RsaKeyPair::generate(KeySize::Rsa2048).expect("a test key")
    }

    #[test]
    fn base64url_matches_the_jwt_alphabet_without_padding() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(base64_decode("Zm9vYmFy").expect("decodes"), b"foobar");
        assert_eq!(base64_decode("Zm9v\nYmE=").expect("decodes"), b"fooba");
        assert!(base64_decode("Zm9v*").is_none());
    }

    #[test]
    fn a_pkcs8_pem_loads_and_the_jwt_verifies_under_its_public_key() {
        let key = test_key();
        let der = key.as_der().expect("pkcs8 der");
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            standard_base64(der.as_ref())
        );
        let loaded = key_pair_from_pem(&pem).expect("the PEM loads");
        let jwt = app_jwt(
            &loaded,
            424242,
            "2026-09-06T10:00:00Z".parse().expect("a time"),
        )
        .expect("a jwt");
        let [header, claims, signature] = jwt.split('.').collect::<Vec<_>>()[..] else {
            panic!("three segments");
        };
        let header_json: serde_json::Value =
            serde_json::from_slice(&url_decode(header)).expect("header json");
        assert_eq!(header_json["alg"], "RS256");
        let claims_json: serde_json::Value =
            serde_json::from_slice(&url_decode(claims)).expect("claims json");
        assert_eq!(claims_json["iss"], "424242");
        assert_eq!(
            claims_json["exp"].as_i64().expect("exp") - claims_json["iat"].as_i64().expect("iat"),
            JWT_BACKDATE_SECS + JWT_LIFETIME_SECS
        );
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, key.public_key().as_ref())
            .verify(
                format!("{header}.{claims}").as_bytes(),
                &url_decode(signature),
            )
            .expect("the signature verifies under the App's public key");
        assert!(
            key_pair_from_pem("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----")
                .is_err()
        );
    }

    #[test]
    fn a_configuration_document_is_validated_structurally() {
        let directory = tempfile::tempdir().expect("a state root");
        assert!(
            GithubPublicationConfig::read(directory.path())
                .expect("absent is valid")
                .is_none()
        );
        std::fs::create_dir_all(directory.path().join("config")).expect("config dir");
        let path = directory.path().join(CONFIG_FILE);
        std::fs::write(&path, r#"{"schema_version":1,"app_id":1,"installation_id":2,"private_key_path":"/k.pem","project_id":"01a0064a-e056-7603-9968-ef64fdaacb75","repositories":["Carasent-ASMA/asma-modules"]}"#).expect("write");
        let config = GithubPublicationConfig::read(directory.path())
            .expect("valid")
            .expect("present");
        assert_eq!(config.check_name, DEFAULT_CHECK_NAME);
        assert_eq!(config.poll_seconds, 60);
        std::fs::write(&path, r#"{"schema_version":1,"app_id":1,"installation_id":2,"private_key_path":"/k.pem","project_id":"01a0064a-e056-7603-9968-ef64fdaacb75","repositories":["not-a-repo"]}"#).expect("write");
        assert!(matches!(
            GithubPublicationConfig::read(directory.path()),
            Err(ConfigError::Invalid { .. })
        ));
        std::fs::write(&path, "{").expect("write");
        assert!(matches!(
            GithubPublicationConfig::read(directory.path()),
            Err(ConfigError::Invalid { .. })
        ));
    }

    #[test]
    fn a_pull_request_maps_to_the_attestation_request_and_a_stable_key() {
        let facts = PullRequestFacts {
            number: 2985,
            title: " ASMA-8101 publication identity enforcement ".to_owned(),
            head: RefFacts {
                name: "feat/ASMA-8101-publication-identity-enforcement".to_owned(),
                sha: "82c56f5043b436ba666962cf82a89e93e330a271".to_owned(),
            },
            base: RefFacts {
                name: "master".to_owned(),
                sha: "0000000000000000000000000000000000000000".to_owned(),
            },
            draft: false,
            state: "open".to_owned(),
            merged: false,
        };
        let request = identity_of("Carasent-ASMA/asma-modules", &facts).expect("maps");
        assert_eq!(request.pull_request, Some(2985));
        assert_eq!(
            request.title.as_ref().map(ExternalName::as_str),
            Some("ASMA-8101 publication identity enforcement")
        );
        assert_eq!(
            check_key("Carasent-ASMA/asma-modules", &facts),
            check_key("Carasent-ASMA/asma-modules", &facts)
        );
        let mut retitled = facts.clone();
        retitled.title = "cat 11".to_owned();
        assert_ne!(
            check_key("Carasent-ASMA/asma-modules", &facts),
            check_key("Carasent-ASMA/asma-modules", &retitled)
        );
        let sha = CommitSha::parse(&facts.head.sha).expect("sha");
        assert_eq!(ensure_mergeable(&facts, &sha), Ok(()));
        let other = CommitSha::parse("0000000000000000000000000000000000000000").expect("sha");
        assert!(matches!(
            ensure_mergeable(&facts, &other),
            Err(MergeRefusal::HeadMoved { .. })
        ));
        assert_eq!(
            ensure_mergeable(
                &PullRequestFacts {
                    draft: true,
                    ..facts.clone()
                },
                &sha
            ),
            Err(MergeRefusal::NotOpen)
        );
    }

    #[tokio::test]
    async fn the_gateway_exchanges_a_jwt_posts_checks_and_merges_by_head() {
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/app/installations/77/access_tokens"))
            .and(header("X-GitHub-Api-Version", API_VERSION))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "token": "ghs_test", "expires_at": "2099-01-01T00:00:00Z"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/Carasent-ASMA/asma-modules/pulls"))
            .and(header("Authorization", "Bearer ghs_test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "number": 2985, "title": "ASMA-8101 x", "draft": false, "state": "open",
                "head": {"ref": "feat/ASMA-8101-x", "sha": "82c56f5043b436ba666962cf82a89e93e330a271"},
                "base": {"ref": "master", "sha": "0000000000000000000000000000000000000000"}
            }])))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/Carasent-ASMA/asma-modules/check-runs"))
            .and(body_partial_json(serde_json::json!({
                "name": DEFAULT_CHECK_NAME, "head_sha": "82c56f5043b436ba666962cf82a89e93e330a271",
                "status": "completed", "conclusion": "failure"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": 1})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/repos/Carasent-ASMA/asma-modules/pulls/2985/merge"))
            .and(body_partial_json(serde_json::json!({"merge_method": "squash", "sha": "82c56f5043b436ba666962cf82a89e93e330a271"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"sha": "abc", "merged": true})))
            .expect(1)
            .mount(&server)
            .await;

        let gateway = GithubPublicationGateway::with_key(
            GithubPublicationConfig {
                schema_version: 1,
                app_id: 424242,
                installation_id: 77,
                private_key_path: PathBuf::from("/unused"),
                project_id: ProjectId::parse("01a0064a-e056-7603-9968-ef64fdaacb75").expect("id"),
                repositories: vec!["Carasent-ASMA/asma-modules".to_owned()],
                check_name: DEFAULT_CHECK_NAME.to_owned(),
                poll_seconds: 60,
                api_base: server.uri(),
            },
            test_key(),
        );
        let pull_requests = gateway
            .open_pull_requests("Carasent-ASMA/asma-modules")
            .await
            .expect("lists");
        assert_eq!(pull_requests[0].number, 2985);
        gateway
            .post_check_run(
                "Carasent-ASMA/asma-modules",
                "82c56f5043b436ba666962cf82a89e93e330a271",
                false,
                "Refused: binding_unconfirmed",
            )
            .await
            .expect("posts");
        let merged = gateway
            .merge_squash(
                "Carasent-ASMA/asma-modules",
                2985,
                "82c56f5043b436ba666962cf82a89e93e330a271",
            )
            .await
            .expect("merges");
        assert_eq!(merged, "abc");
        // Three calls, one token exchange: the cached token was reused.
        assert!(gateway.governs("carasent-asma/ASMA-MODULES"));
        assert!(!gateway.governs("Carasent-ASMA/other"));
    }

    fn standard_base64(bytes: &[u8]) -> String {
        let mut out = base64url(bytes).replace('-', "+").replace('_', "/");
        while !out.len().is_multiple_of(4) {
            out.push('=');
        }
        out
    }

    fn url_decode(segment: &str) -> Vec<u8> {
        base64_decode(&segment.replace('-', "+").replace('_', "/")).expect("a base64url segment")
    }
}
