//! The read-only Jira seam for observational catalog collection.
//!
//! This is deliberately **not** [`crate::JiraConnector`]. The governed
//! connector is project-keyed, privately constructed, and its authenticated
//! operation can transition, assign and edit. The catalog needs the opposite
//! shape: one explicitly configured scope, GET requests only, and an interface
//! that cannot carry a mutation even by mistake.
//!
//! What is shared with the governed path is the part that should never diverge:
//! [`crate::credentials::JiraCredentialScope`] addresses the keychain entry,
//! [`crate::credentials::parse_credentials`] validates the stored document,
//! redirects are refused rather than followed, responses are bounded, and the
//! same connect/request time budgets apply. Those are properties of the
//! credential and the transport, not of the authority to write.
//!
//! Pagination uses Jira's enhanced search (`/rest/api/3/search/jql`) with
//! `nextPageToken`. Retries are bounded and `Retry-After` is honoured; the
//! caller receives the observed backoff and request counts so it can record
//! coverage rather than guess. Search consistency lag is an input here, never
//! an excuse to report a deletion: absence from a search page is not absence
//! of an issue.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use kontor_accounts::{KeychainBackend, SystemKeychain};
use kontor_core::id::RealmId;
use reqwest::header::{ACCEPT, RETRY_AFTER};
use reqwest::{Client, Method, StatusCode, Url};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::form_urlencoded;

use crate::credentials::{JiraCredentialScope, parse_credentials, validate_alias};

/// The default per-response byte bound. It matches the governed connector's
/// independent bound; a catalog page is far smaller.
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// The default connect bound.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The default whole-request bound.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// The default attempt bound, including the first attempt.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// The default first retry delay.
pub const DEFAULT_RETRY_BASE: Duration = Duration::from_millis(250);
/// The largest exponential delay this seam will sleep through.
pub const DEFAULT_RETRY_MAX: Duration = Duration::from_secs(5);
/// A `Retry-After` beyond this bound is surfaced to the caller instead of being
/// slept through. The caller's cadence and coverage are the right place for a
/// long backoff; a hidden thirty-minute sleep inside one request is not.
pub const DEFAULT_RETRY_AFTER_MAX: Duration = Duration::from_secs(30);
/// The default enhanced-search page size, which is also Jira's maximum.
pub const DEFAULT_PAGE_SIZE: u32 = 100;
/// The default bound on a single logical search.
pub const DEFAULT_MAX_PAGES: usize = 200;
/// The default bound on issues returned by one logical search.
pub const DEFAULT_MAX_ISSUES: usize = 5000;

const MAX_PROJECT_KEY: usize = 80;
const MAX_SCOPE_JQL: usize = 2000;
const MAX_KEY: usize = 64;
const MAX_ISSUE_ID: usize = 20;
const MAX_SUMMARY: usize = 1000;
const MAX_TYPE: usize = 80;
const MAX_STATUS: usize = 160;
const MAX_LABELS: usize = 100;
const MAX_LABEL: usize = 255;
const MAX_CREDENTIAL_TIMEOUT: Duration = Duration::from_secs(30);

/// The read-only request limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JiraReadLimits {
    /// The maximum response body in bytes.
    pub max_response_bytes: usize,
    /// The TCP connect bound.
    pub connect_timeout: Duration,
    /// The whole-request bound.
    pub request_timeout: Duration,
    /// The maximum issues returned by one logical search.
    pub max_issues: usize,
    /// The maximum pages walked by one logical search.
    pub max_pages: usize,
    /// The enhanced-search page size, clamped to `1..=100`.
    pub page_size: u32,
}

impl Default for JiraReadLimits {
    fn default() -> Self {
        Self {
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_issues: DEFAULT_MAX_ISSUES,
            max_pages: DEFAULT_MAX_PAGES,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }
}

/// The bounded retry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JiraRetryPolicy {
    /// The total attempts including the first, in `1..=10`.
    pub max_attempts: u32,
    /// The first exponential delay.
    pub base_delay: Duration,
    /// The exponential delay ceiling.
    pub max_delay: Duration,
    /// The largest `Retry-After` this seam will sleep through. A larger value
    /// is returned to the caller with its absolute deadline.
    pub max_retry_after: Duration,
}

impl Default for JiraRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            base_delay: DEFAULT_RETRY_BASE,
            max_delay: DEFAULT_RETRY_MAX,
            max_retry_after: DEFAULT_RETRY_AFTER_MAX,
        }
    }
}

/// The explicit read-only scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraReadConfig {
    /// The Jira base URL. HTTPS, unless explicit loopback HTTP for a fixture.
    pub endpoint: String,
    /// The expected project key. Every observed issue is checked against it by
    /// the collector; the seam uses it only to validate the configuration.
    pub project_key: String,
    /// The configured scope JQL. It is sent verbatim and bounded.
    pub scope_jql: String,
    /// The keychain alias, never the secret.
    pub credential_alias: String,
    /// Request limits.
    pub limits: JiraReadLimits,
    /// Retry policy.
    pub retry: JiraRetryPolicy,
}

impl JiraReadConfig {
    /// Build a configuration with the default limits and retry policy.
    #[must_use]
    pub fn new(
        endpoint: impl Into<String>,
        project_key: impl Into<String>,
        scope_jql: impl Into<String>,
        credential_alias: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            project_key: project_key.into(),
            scope_jql: scope_jql.into(),
            credential_alias: credential_alias.into(),
            limits: JiraReadLimits::default(),
            retry: JiraRetryPolicy::default(),
        }
    }
}

/// Why a read-only request could not produce an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraReadFailure {
    /// The keychain entry is absent, unreadable, or not the supported document.
    Credential,
    /// Jira answered 401 or 403. Permission loss is not deletion.
    Permission,
    /// Jira answered 404 for an addressed resource.
    NotFound,
    /// A 429 or exhausted 5xx retry.
    RateLimited,
    /// The request exceeded its bound.
    Timeout,
    /// The transport failed before a usable answer.
    Transport,
    /// The response exceeded the byte bound.
    Oversized,
    /// The response did not have the supported shape.
    Malformed,
    /// Jira rejected the request itself (for example a bad JQL or key list).
    Rejected,
    /// A page or issue bound stopped the logical search.
    Bounded,
    /// The configuration could not form one safe request.
    Configuration,
}

impl JiraReadFailure {
    /// The stable machine spelling for coverage and diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Credential => "credential",
            Self::Permission => "permission",
            Self::NotFound => "not_found",
            Self::RateLimited => "rate_limited",
            Self::Timeout => "timeout",
            Self::Transport => "transport",
            Self::Oversized => "oversized",
            Self::Malformed => "malformed",
            Self::Rejected => "rejected",
            Self::Bounded => "bounded",
            Self::Configuration => "configuration",
        }
    }
}

impl std::fmt::Display for JiraReadFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What one logical read attempted, for honest coverage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JiraReadStats {
    /// Attempts made, including retries.
    pub requests: usize,
    /// Retries actually slept and repeated.
    pub retries: usize,
    /// Enhanced-search pages read.
    pub pages: usize,
    /// 429 answers observed.
    pub rate_limited: usize,
    /// 404 answers observed and treated as "not found", never "deleted".
    pub not_found: usize,
    /// The last HTTP status observed.
    pub last_status: Option<u16>,
    /// The last `Retry-After` value observed, in seconds.
    pub retry_after_seconds: Option<u64>,
    /// The absolute deadline of the last backoff, when one was observed.
    pub backoff_until: Option<String>,
}

impl JiraReadStats {
    /// Fold another read's counters into this one.
    pub fn merge(&mut self, other: &Self) {
        self.requests = self.requests.saturating_add(other.requests);
        self.retries = self.retries.saturating_add(other.retries);
        self.pages = self.pages.saturating_add(other.pages);
        self.rate_limited = self.rate_limited.saturating_add(other.rate_limited);
        self.not_found = self.not_found.saturating_add(other.not_found);
        self.last_status = other.last_status.or(self.last_status);
        self.retry_after_seconds = other.retry_after_seconds.or(self.retry_after_seconds);
        if let Some(until) = other.backoff_until.as_ref()
            && self
                .backoff_until
                .as_deref()
                .is_none_or(|current| until.as_str() > current)
        {
            self.backoff_until = Some(until.clone());
        }
    }
}

/// One bounded read error. The detail is structural and never contains a
/// response body, a credential or a request header.
#[derive(Debug, Clone)]
pub struct JiraReadError {
    /// The class a caller branches on.
    pub failure: JiraReadFailure,
    /// The HTTP status, when one was observed.
    pub status: Option<u16>,
    /// The last `Retry-After`, when one was observed.
    pub retry_after_seconds: Option<u64>,
    /// The absolute backoff deadline, when one was observed.
    pub backoff_until: Option<String>,
    /// The counters observed before the error.
    pub stats: Box<JiraReadStats>,
    detail: &'static str,
}

impl JiraReadError {
    fn new(failure: JiraReadFailure, stats: JiraReadStats, detail: &'static str) -> Self {
        Self {
            failure,
            status: stats.last_status,
            retry_after_seconds: stats.retry_after_seconds,
            backoff_until: stats.backoff_until.clone(),
            stats: Box::new(stats),
            detail,
        }
    }

    fn with_status(
        failure: JiraReadFailure,
        status: u16,
        stats: JiraReadStats,
        detail: &'static str,
    ) -> Self {
        let mut stats = stats;
        stats.last_status = Some(status);
        Self::new(failure, stats, detail)
    }

    /// A structural explanation. It is safe for a diagnostic.
    #[must_use]
    pub const fn detail(&self) -> &'static str {
        self.detail
    }

    /// Whether this failure leaves prior observations uncertain but not gone.
    #[must_use]
    pub const fn is_uncertain(&self) -> bool {
        matches!(
            self.failure,
            JiraReadFailure::Credential
                | JiraReadFailure::Permission
                | JiraReadFailure::RateLimited
                | JiraReadFailure::Timeout
                | JiraReadFailure::Transport
                | JiraReadFailure::Oversized
                | JiraReadFailure::Malformed
                | JiraReadFailure::Bounded
        )
    }
}

impl std::fmt::Display for JiraReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "jira read failed: {} ({})",
            self.failure, self.detail
        )
    }
}

impl std::error::Error for JiraReadError {}

/// One issue exactly as Jira answered it, with no inferred field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraReadIssue {
    /// Jira's immutable REST id.
    pub id: String,
    /// The issue's current key.
    pub key: String,
    /// The actual issue type name.
    pub issue_type: String,
    /// The actual hierarchy level.
    pub level: i64,
    /// The parent's current key, when Jira names one.
    pub parent_key: Option<String>,
    /// The parent's immutable id, when Jira names one.
    pub parent_id: Option<String>,
    /// Every label, sorted for a stable answer.
    pub labels: Vec<String>,
    /// The status name.
    pub status: String,
    /// The status category key: `new`, `indeterminate` or `done`.
    pub status_category: String,
    /// The source's update instant, normalized to canonical UTC.
    pub updated: String,
    /// The summary, bounded.
    pub summary: String,
}

/// One enhanced-search page.
#[derive(Debug, Clone)]
pub struct JiraSearchPage {
    /// The issues on this page.
    pub issues: Vec<JiraReadIssue>,
    /// The next page token, when there is one.
    pub next_page_token: Option<String>,
    /// Whether Jira said this is the last page.
    pub is_last: bool,
    /// Counters for this page.
    pub stats: JiraReadStats,
}

/// One complete logical search.
#[derive(Debug, Clone)]
pub struct JiraSearch {
    /// Every issue, deduplicated by immutable id.
    pub issues: Vec<JiraReadIssue>,
    /// The number of pages read.
    pub pages: usize,
    /// Counters for the whole search.
    pub stats: JiraReadStats,
}

/// The answer to one direct issue read.
#[derive(Debug, Clone)]
pub struct JiraIssueAnswer {
    /// `None` means Jira answered 404. It never means "deleted".
    pub issue: Option<JiraReadIssue>,
    /// Counters for this read.
    pub stats: JiraReadStats,
}

/// Proof that the credential resolves and Jira accepts one authenticated read.
#[derive(Debug, Clone, Default)]
pub struct JiraPreflight {
    /// Counters for the preflight read.
    pub stats: JiraReadStats,
}

/// The read-only Jira client.
///
/// It has no method that can issue a Jira write. The credential is resolved for
/// each request and never retained in a public field or a `Debug` rendering.
pub struct JiraReadOnly {
    endpoint: Url,
    project_key: String,
    scope_jql: String,
    credential_alias: String,
    credential_scope: JiraCredentialScope,
    keychain: Arc<dyn KeychainBackend>,
    client: Client,
    limits: JiraReadLimits,
    retry: JiraRetryPolicy,
}

impl std::fmt::Debug for JiraReadOnly {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JiraReadOnly")
            .field("origin", &self.endpoint.origin().ascii_serialization())
            .field("project_key", &self.project_key)
            .field("credential", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl JiraReadOnly {
    /// Compose the production seam against the OS keychain.
    ///
    /// # Errors
    /// A refused configuration or an unusable credential scope.
    pub fn new(
        state_root: &Path,
        realm: RealmId,
        config: JiraReadConfig,
    ) -> Result<Self, JiraReadError> {
        Self::new_with_keychain(state_root, realm, config, Arc::new(SystemKeychain))
    }

    /// The same seam with an injected keychain. This is the qualification seam
    /// the governed connector already establishes; it does not change what the
    /// client may do.
    ///
    /// # Errors
    /// A refused configuration or an unusable credential scope.
    pub fn new_with_keychain(
        state_root: &Path,
        realm: RealmId,
        config: JiraReadConfig,
        keychain: Arc<dyn KeychainBackend>,
    ) -> Result<Self, JiraReadError> {
        let credential_scope = JiraCredentialScope::at(state_root, realm).map_err(|_| {
            JiraReadError::new(
                JiraReadFailure::Credential,
                JiraReadStats::default(),
                "the configured credential scope could not be composed",
            )
        })?;
        validate_alias(&config.credential_alias).map_err(|_| {
            JiraReadError::new(
                JiraReadFailure::Configuration,
                JiraReadStats::default(),
                "the credential alias is empty, oversized or unsupported",
            )
        })?;
        let project_key = config.project_key.trim().to_owned();
        if project_key.is_empty()
            || project_key.len() > MAX_PROJECT_KEY
            || !project_key.bytes().enumerate().all(|(index, byte)| {
                if index == 0 {
                    byte.is_ascii_uppercase()
                } else {
                    byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                }
            })
        {
            return Err(JiraReadError::new(
                JiraReadFailure::Configuration,
                JiraReadStats::default(),
                "the project key is not an uppercase Jira project key",
            ));
        }
        let scope_jql = config.scope_jql.trim().to_owned();
        if scope_jql.is_empty()
            || scope_jql.chars().count() > MAX_SCOPE_JQL
            || scope_jql.chars().any(char::is_control)
        {
            return Err(JiraReadError::new(
                JiraReadFailure::Configuration,
                JiraReadStats::default(),
                "the scope JQL is empty, oversized or contains a control character",
            ));
        }
        let limits = validated_limits(config.limits)?;
        let retry = validated_retry(config.retry)?;
        let endpoint = validated_endpoint(&config.endpoint)?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(limits.connect_timeout)
            .timeout(limits.request_timeout)
            .build()
            .map_err(|_| {
                JiraReadError::new(
                    JiraReadFailure::Configuration,
                    JiraReadStats::default(),
                    "the Jira read client could not be built",
                )
            })?;
        Ok(Self {
            endpoint,
            project_key,
            scope_jql,
            credential_alias: config.credential_alias,
            credential_scope,
            keychain,
            client,
            limits,
            retry,
        })
    }

    /// The configured origin, without credentials.
    #[must_use]
    pub fn origin(&self) -> String {
        self.endpoint.origin().ascii_serialization()
    }

    /// The configured project key.
    #[must_use]
    pub fn project_key(&self) -> &str {
        &self.project_key
    }

    /// The configured scope JQL.
    #[must_use]
    pub fn scope_jql(&self) -> &str {
        &self.scope_jql
    }

    /// The credential alias, never the secret.
    #[must_use]
    pub fn credential_reference(&self) -> &str {
        &self.credential_alias
    }

    /// Prove the keychain entry resolves and Jira accepts an authenticated
    /// read. The response body is discarded.
    ///
    /// # Errors
    /// A credential, permission or transport refusal.
    pub async fn preflight(&self) -> Result<JiraPreflight, JiraReadError> {
        let (_, stats) = self.get_json("rest/api/3/myself", &[]).await?;
        Ok(JiraPreflight { stats })
    }

    /// Walk every page of one enhanced search.
    ///
    /// # Errors
    /// Any request refusal, a repeated page token, or a page/issue bound.
    pub async fn search_all(&self, jql: &str) -> Result<JiraSearch, JiraReadError> {
        if jql.trim().is_empty()
            || jql.chars().count() > MAX_SCOPE_JQL
            || jql.chars().any(char::is_control)
        {
            return Err(JiraReadError::new(
                JiraReadFailure::Configuration,
                JiraReadStats::default(),
                "the search JQL is empty, oversized or contains a control character",
            ));
        }
        let mut token: Option<String> = None;
        let mut seen_tokens = BTreeSet::new();
        let mut issues: Vec<JiraReadIssue> = Vec::new();
        let mut seen_ids = BTreeSet::new();
        let mut stats = JiraReadStats::default();
        let mut pages = 0usize;
        loop {
            if pages >= self.limits.max_pages {
                return Err(JiraReadError::new(
                    JiraReadFailure::Bounded,
                    stats,
                    "the configured page bound stopped the search",
                ));
            }
            let page = self.search_page(jql, token.as_deref()).await?;
            stats.merge(&page.stats);
            pages += 1;
            for issue in page.issues {
                if !seen_ids.insert(issue.id.clone()) {
                    continue;
                }
                if issues.len() >= self.limits.max_issues {
                    return Err(JiraReadError::new(
                        JiraReadFailure::Bounded,
                        stats,
                        "the configured issue bound stopped the search",
                    ));
                }
                issues.push(issue);
            }
            if page.is_last {
                break;
            }
            match page.next_page_token {
                Some(next) if !next.is_empty() => {
                    if !seen_tokens.insert(next.clone()) {
                        return Err(JiraReadError::new(
                            JiraReadFailure::Malformed,
                            stats,
                            "Jira repeated a search page token",
                        ));
                    }
                    token = Some(next);
                }
                _ => {
                    return Err(JiraReadError::new(
                        JiraReadFailure::Malformed,
                        stats,
                        "Jira did not supply a continuation token for a non-final search page",
                    ));
                }
            }
        }
        stats.pages = pages;
        Ok(JiraSearch {
            issues,
            pages,
            stats,
        })
    }

    /// Read one enhanced-search page.
    ///
    /// # Errors
    /// Any request refusal or a malformed page.
    pub async fn search_page(
        &self,
        jql: &str,
        next_page_token: Option<&str>,
    ) -> Result<JiraSearchPage, JiraReadError> {
        let mut query = vec![
            ("jql".to_owned(), jql.to_owned()),
            ("fields".to_owned(), READ_FIELDS.to_owned()),
            ("maxResults".to_owned(), self.limits.page_size.to_string()),
        ];
        if let Some(token) = next_page_token {
            query.push(("nextPageToken".to_owned(), token.to_owned()));
        }
        let (value, stats) = self.get_json("rest/api/3/search/jql", &query).await?;
        let raw_issues = value
            .get("issues")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                JiraReadError::new(
                    JiraReadFailure::Malformed,
                    stats.clone(),
                    "the search answer has no issues array",
                )
            })?;
        let mut issues = Vec::with_capacity(raw_issues.len());
        for raw in raw_issues {
            issues.push(parse_issue(raw).map_err(|error| {
                let mut error = error;
                error.stats.merge(&stats);
                error
            })?);
        }
        let next = value
            .get("nextPageToken")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned);
        let is_last = value
            .get("isLast")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(JiraSearchPage {
            issues,
            next_page_token: next,
            is_last,
            stats,
        })
    }

    /// Read one issue by its current or historical key.
    ///
    /// A 404 is `Ok(None)`: Jira answers the same for a deleted issue and one
    /// this credential may not read, so the catalog may never turn it into a
    /// deletion.
    ///
    /// # Errors
    /// A credential, permission or transport refusal.
    pub async fn issue(&self, key: &str) -> Result<JiraIssueAnswer, JiraReadError> {
        if !valid_issue_key(key) {
            return Err(JiraReadError::new(
                JiraReadFailure::Configuration,
                JiraReadStats::default(),
                "the issue key is not a Jira issue key",
            ));
        }
        let encoded = form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>();
        let query = vec![("fields".to_owned(), READ_FIELDS.to_owned())];
        match self
            .get_json(&format!("rest/api/3/issue/{encoded}"), &query)
            .await
        {
            Ok((value, stats)) => {
                let issue = parse_issue(&value).map_err(|mut error| {
                    error.stats.merge(&stats);
                    error
                })?;
                Ok(JiraIssueAnswer {
                    issue: Some(issue),
                    stats,
                })
            }
            Err(error) if error.failure == JiraReadFailure::NotFound => {
                let mut stats = *error.stats;
                stats.not_found = stats.not_found.saturating_add(1);
                Ok(JiraIssueAnswer { issue: None, stats })
            }
            Err(error) => Err(error),
        }
    }

    async fn credentials(&self) -> Result<crate::credentials::JiraCredentials, JiraReadError> {
        let keychain = Arc::clone(&self.keychain);
        let target = self.credential_scope.target(&self.credential_alias);
        let secret = tokio::time::timeout(
            MAX_CREDENTIAL_TIMEOUT,
            tokio::task::spawn_blocking(move || keychain.secret(&target)),
        )
        .await
        .map_err(|_| {
            JiraReadError::new(
                JiraReadFailure::Credential,
                JiraReadStats::default(),
                "the keychain credential read exceeded its bound",
            )
        })?
        .map_err(|_| {
            JiraReadError::new(
                JiraReadFailure::Credential,
                JiraReadStats::default(),
                "the keychain credential read was cancelled",
            )
        })?
        .map_err(|_| {
            JiraReadError::new(
                JiraReadFailure::Credential,
                JiraReadStats::default(),
                "the configured keychain credential could not be resolved",
            )
        })?;
        parse_credentials(&secret).map_err(|_| {
            JiraReadError::new(
                JiraReadFailure::Credential,
                JiraReadStats::default(),
                "the keychain credential is not the supported document",
            )
        })
    }

    fn url(&self, path: &str) -> Result<Url, JiraReadError> {
        let url = self.endpoint.join(path).map_err(|_| {
            JiraReadError::new(
                JiraReadFailure::Configuration,
                JiraReadStats::default(),
                "a Jira read path could not be formed",
            )
        })?;
        if url.origin() != self.endpoint.origin() {
            return Err(JiraReadError::new(
                JiraReadFailure::Configuration,
                JiraReadStats::default(),
                "a Jira read may not leave the configured origin",
            ));
        }
        Ok(url)
    }

    async fn get_json(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<(Value, JiraReadStats), JiraReadError> {
        let mut stats = JiraReadStats::default();
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            stats.requests = stats.requests.saturating_add(1);
            let result = self.send_once(path, query).await;
            match result {
                Ok(response) => {
                    let status = response.status();
                    stats.last_status = Some(status.as_u16());
                    if status.is_redirection() {
                        return Err(JiraReadError::with_status(
                            JiraReadFailure::Rejected,
                            status.as_u16(),
                            stats,
                            "Jira redirects are not followed",
                        ));
                    }
                    if status == StatusCode::UNAUTHORIZED {
                        return Err(JiraReadError::with_status(
                            JiraReadFailure::Permission,
                            status.as_u16(),
                            stats,
                            "Jira rejected the configured credential",
                        ));
                    }
                    if status == StatusCode::FORBIDDEN {
                        return Err(JiraReadError::with_status(
                            JiraReadFailure::Permission,
                            status.as_u16(),
                            stats,
                            "Jira refused this credential the read permission",
                        ));
                    }
                    if status == StatusCode::NOT_FOUND {
                        return Err(JiraReadError::with_status(
                            JiraReadFailure::NotFound,
                            status.as_u16(),
                            stats,
                            "Jira does not answer for this address",
                        ));
                    }
                    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                        let retry_after = response
                            .headers()
                            .get(RETRY_AFTER)
                            .and_then(|value| value.to_str().ok())
                            .and_then(parse_retry_after);
                        stats.retry_after_seconds = retry_after;
                        if status == StatusCode::TOO_MANY_REQUESTS {
                            stats.rate_limited = stats.rate_limited.saturating_add(1);
                        }
                        let delay = retry_after
                            .map(Duration::from_secs)
                            .unwrap_or_else(|| self.exponential_delay(attempt));
                        let backoff_until = backoff_until(delay);
                        stats.backoff_until = Some(backoff_until);
                        if retry_after.is_some_and(|seconds| {
                            Duration::from_secs(seconds) > self.retry.max_retry_after
                        }) || attempt >= self.retry.max_attempts
                        {
                            return Err(JiraReadError::with_status(
                                JiraReadFailure::RateLimited,
                                status.as_u16(),
                                stats,
                                "Jira is rate limiting or unavailable and the retry bound is spent",
                            ));
                        }
                        stats.retries = stats.retries.saturating_add(1);
                        tokio::time::sleep(delay.min(self.retry.max_retry_after)).await;
                        continue;
                    }
                    if status == StatusCode::BAD_REQUEST {
                        return Err(JiraReadError::with_status(
                            JiraReadFailure::Rejected,
                            status.as_u16(),
                            stats,
                            "Jira rejected the read request",
                        ));
                    }
                    if !status.is_success() {
                        return Err(JiraReadError::with_status(
                            JiraReadFailure::Transport,
                            status.as_u16(),
                            stats,
                            "Jira returned a non-success status",
                        ));
                    }
                    let value = self.read_body(response, &mut stats).await?;
                    return Ok((value, stats));
                }
                Err(error) => {
                    stats.last_status = error.status;
                    stats.backoff_until = error.backoff_until.clone();
                    match error.failure {
                        JiraReadFailure::Timeout | JiraReadFailure::Transport
                            if attempt < self.retry.max_attempts =>
                        {
                            let delay = self.exponential_delay(attempt);
                            stats.retries = stats.retries.saturating_add(1);
                            stats.backoff_until = Some(backoff_until(delay));
                            tokio::time::sleep(delay.min(self.retry.max_delay)).await;
                            continue;
                        }
                        JiraReadFailure::Timeout => {
                            return Err(JiraReadError::new(
                                JiraReadFailure::Timeout,
                                stats,
                                "the Jira read exceeded its request bound",
                            ));
                        }
                        JiraReadFailure::Transport => {
                            return Err(JiraReadError::new(
                                JiraReadFailure::Transport,
                                stats,
                                "the Jira transport failed",
                            ));
                        }
                        JiraReadFailure::Credential => {
                            return Err(JiraReadError::new(
                                JiraReadFailure::Credential,
                                stats,
                                "the configured credential could not be resolved",
                            ));
                        }
                        other => {
                            return Err(JiraReadError::new(other, stats, error.detail()));
                        }
                    }
                }
            }
        }
    }

    async fn send_once(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<reqwest::Response, JiraReadError> {
        let credentials = self.credentials().await?;
        let mut url = self.url(path)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let request = self
            .client
            .request(Method::GET, url)
            .basic_auth(
                credentials.email.expose_secret(),
                Some(credentials.api_token.expose_secret()),
            )
            .header(ACCEPT, "application/json");
        request.send().await.map_err(|error| {
            let failure = if error.is_timeout() {
                JiraReadFailure::Timeout
            } else {
                JiraReadFailure::Transport
            };
            JiraReadError::new(
                failure,
                JiraReadStats::default(),
                if failure == JiraReadFailure::Timeout {
                    "the Jira read exceeded its request bound"
                } else {
                    "the Jira transport failed"
                },
            )
        })
    }

    async fn read_body(
        &self,
        response: reqwest::Response,
        stats: &mut JiraReadStats,
    ) -> Result<Value, JiraReadError> {
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|length| length > self.limits.max_response_bytes as u64)
        {
            return Err(JiraReadError::with_status(
                JiraReadFailure::Oversized,
                status,
                stats.clone(),
                "Jira returned an oversized response",
            ));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| {
                JiraReadError::with_status(
                    JiraReadFailure::Transport,
                    status,
                    stats.clone(),
                    "the Jira response body failed",
                )
            })?;
            if bytes.len().saturating_add(chunk.len()) > self.limits.max_response_bytes {
                return Err(JiraReadError::with_status(
                    JiraReadFailure::Oversized,
                    status,
                    stats.clone(),
                    "Jira returned an oversized response",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            JiraReadError::with_status(
                JiraReadFailure::Malformed,
                status,
                stats.clone(),
                "Jira returned a malformed JSON response",
            )
        })
    }

    fn exponential_delay(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(16);
        let factor = 1u32 << exponent;
        self.retry
            .base_delay
            .saturating_mul(factor)
            .min(self.retry.max_delay)
    }
}

const READ_FIELDS: &str = "summary,status,issuetype,parent,labels,updated";

fn validated_limits(limits: JiraReadLimits) -> Result<JiraReadLimits, JiraReadError> {
    if limits.max_response_bytes < 1024
        || limits.max_response_bytes > 64 * 1024 * 1024
        || limits.connect_timeout.is_zero()
        || limits.request_timeout.is_zero()
        || limits.max_issues == 0
        || limits.max_pages == 0
        || limits.page_size == 0
    {
        return Err(JiraReadError::new(
            JiraReadFailure::Configuration,
            JiraReadStats::default(),
            "a read limit is outside its supported range",
        ));
    }
    Ok(JiraReadLimits {
        page_size: limits.page_size.min(100),
        ..limits
    })
}

fn validated_retry(retry: JiraRetryPolicy) -> Result<JiraRetryPolicy, JiraReadError> {
    if retry.max_attempts == 0
        || retry.max_attempts > 10
        || retry.base_delay.is_zero()
        || retry.max_delay < retry.base_delay
        || retry.max_retry_after.is_zero()
    {
        return Err(JiraReadError::new(
            JiraReadFailure::Configuration,
            JiraReadStats::default(),
            "the retry policy is outside its supported range",
        ));
    }
    Ok(retry)
}

fn validated_endpoint(endpoint: &str) -> Result<Url, JiraReadError> {
    let mut endpoint = Url::parse(endpoint).map_err(|_| {
        JiraReadError::new(
            JiraReadFailure::Configuration,
            JiraReadStats::default(),
            "the endpoint is not an absolute URL",
        )
    })?;
    let loopback = endpoint
        .host_str()
        .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1"));
    if endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && loopback) {
        return Err(JiraReadError::new(
            JiraReadFailure::Configuration,
            JiraReadStats::default(),
            "the endpoint must use HTTPS (or explicit loopback HTTP)",
        ));
    }
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(JiraReadError::new(
            JiraReadFailure::Configuration,
            JiraReadStats::default(),
            "the endpoint may not contain credentials, query or fragment",
        ));
    }
    endpoint.set_path("/");
    Ok(endpoint)
}

fn parse_issue(value: &Value) -> Result<JiraReadIssue, JiraReadError> {
    let malformed = |detail: &'static str| {
        JiraReadError::new(JiraReadFailure::Malformed, JiraReadStats::default(), detail)
    };
    let id = text_at(value, &["id"]).ok_or_else(|| malformed("the issue id is missing"))?;
    if id.is_empty()
        || id.len() > MAX_ISSUE_ID
        || !id.bytes().all(|byte| byte.is_ascii_digit())
        || id.starts_with('0')
    {
        return Err(malformed("the issue id is not an immutable Jira id"));
    }
    let key = text_at(value, &["key"]).ok_or_else(|| malformed("the issue key is missing"))?;
    if !valid_issue_key(key) {
        return Err(malformed("the issue key is not a Jira issue key"));
    }
    let fields = value
        .get("fields")
        .ok_or_else(|| malformed("the issue fields are missing"))?;
    let issue_type = text_at(fields, &["issuetype", "name"])
        .ok_or_else(|| malformed("the issue type is missing"))?;
    if issue_type.is_empty() || issue_type.chars().count() > MAX_TYPE {
        return Err(malformed("the issue type is empty or oversized"));
    }
    let level = fields
        .get("issuetype")
        .and_then(|value| value.get("hierarchyLevel"))
        .and_then(Value::as_i64)
        .ok_or_else(|| malformed("the issue hierarchy level is missing"))?;
    if !(-1..=9).contains(&level) {
        return Err(malformed("the issue hierarchy level is unsupported"));
    }
    let parent_key = optional_text_at(fields, &["parent", "key"]);
    if parent_key.is_some_and(|key| !valid_issue_key(key)) {
        return Err(malformed("the parent key is not a Jira issue key"));
    }
    let parent_id = optional_text_at(fields, &["parent", "id"]);
    if parent_id.is_some_and(|id| {
        id.is_empty()
            || id.len() > MAX_ISSUE_ID
            || !id.bytes().all(|byte| byte.is_ascii_digit())
            || id.starts_with('0')
    }) {
        return Err(malformed("the parent id is not an immutable Jira id"));
    }
    let raw_labels = fields
        .get("labels")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("the issue labels are missing"))?;
    if raw_labels.len() > MAX_LABELS {
        return Err(malformed("the issue has too many labels"));
    }
    let mut labels = BTreeSet::new();
    for label in raw_labels {
        let label = label
            .as_str()
            .ok_or_else(|| malformed("an issue label is not text"))?;
        if label.is_empty() || label.chars().count() > MAX_LABEL {
            return Err(malformed("an issue label is empty or oversized"));
        }
        labels.insert(label.to_owned());
    }
    let status =
        text_at(fields, &["status", "name"]).ok_or_else(|| malformed("the status is missing"))?;
    if status.is_empty() || status.chars().count() > MAX_STATUS {
        return Err(malformed("the status is empty or oversized"));
    }
    let status_category = text_at(fields, &["status", "statusCategory", "key"])
        .ok_or_else(|| malformed("the status category is missing"))?;
    if !matches!(status_category, "new" | "indeterminate" | "done") {
        return Err(malformed("the status category is unsupported"));
    }
    let updated =
        text_at(fields, &["updated"]).ok_or_else(|| malformed("the update instant is missing"))?;
    let updated = normalize_jira_instant(updated)
        .ok_or_else(|| malformed("the update instant is not a supported Jira instant"))?;
    let summary =
        text_at(fields, &["summary"]).ok_or_else(|| malformed("the issue summary is missing"))?;
    if summary.chars().count() > MAX_SUMMARY {
        return Err(malformed("the issue summary is oversized"));
    }
    Ok(JiraReadIssue {
        id: id.to_owned(),
        key: key.to_owned(),
        issue_type: issue_type.to_owned(),
        level,
        parent_key: parent_key.map(ToOwned::to_owned),
        parent_id: parent_id.map(ToOwned::to_owned),
        labels: labels.into_iter().collect(),
        status: status.to_owned(),
        status_category: status_category.to_owned(),
        updated,
        summary: summary.to_owned(),
    })
}

fn text_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(Value::as_str)
}

fn optional_text_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    text_at(value, path).filter(|text| !text.is_empty())
}

/// Normalize Jira's `2026-10-24T18:05:12.345+0200` to canonical UTC.
fn normalize_jira_instant(raw: &str) -> Option<String> {
    let mut text = raw.trim().to_owned();
    if text.is_empty() || text.len() > 64 {
        return None;
    }
    if let Some(position) = text.rfind(['+', '-']) {
        let suffix = &text[position..];
        if suffix.len() == 5 && suffix[1..].bytes().all(|byte| byte.is_ascii_digit()) {
            text = format!("{}{}:{}", &text[..position], &suffix[..3], &suffix[3..]);
        }
    }
    let instant: kontor_core::id::Timestamp = text.parse().ok()?;
    Some(instant.to_string())
}

fn valid_issue_key(key: &str) -> bool {
    if key.len() > MAX_KEY {
        return false;
    }
    let Some((project, number)) = key.rsplit_once('-') else {
        return false;
    };
    if project.is_empty()
        || project.len() > MAX_PROJECT_KEY
        || !project.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_uppercase()
            } else {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
            }
        })
    {
        return false;
    }
    !number.is_empty()
        && !number.starts_with('0')
        && number.len() <= MAX_ISSUE_ID
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn parse_retry_after(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }
    let deadline = httpdate::parse_http_date(value).ok()?;
    let now = std::time::SystemTime::now();
    deadline
        .duration_since(now)
        .ok()
        .map(|duration| duration.as_secs())
}

fn backoff_until(delay: Duration) -> String {
    let now = kontor_core::id::Timestamp::now();
    let seconds = i64::try_from(delay.as_secs()).unwrap_or(i64::MAX);
    (now + kontor_core::id::SignedDuration::from_secs(seconds)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jira_instants_normalize_to_canonical_utc() {
        assert_eq!(
            normalize_jira_instant("2026-10-24T18:05:12.345+0200").as_deref(),
            Some("2026-10-24T16:05:12.345Z")
        );
        assert_eq!(
            normalize_jira_instant("2026-10-24T18:05:12.345+02:00").as_deref(),
            Some("2026-10-24T16:05:12.345Z")
        );
        assert_eq!(
            normalize_jira_instant("2026-10-24T16:05:12Z").as_deref(),
            Some("2026-10-24T16:05:12Z")
        );
        assert!(normalize_jira_instant("yesterday").is_none());
    }

    #[test]
    fn issue_keys_are_exact() {
        assert!(valid_issue_key("ASMA-1"));
        assert!(valid_issue_key("ASMA_2-999"));
        assert!(!valid_issue_key("ASMA-0"));
        assert!(!valid_issue_key("ASMA-01"));
        assert!(!valid_issue_key("asma-1"));
        assert!(!valid_issue_key("-1"));
        assert!(!valid_issue_key("ASMA-1-2"));
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_dates() {
        assert_eq!(parse_retry_after("3"), Some(3));
        let later = std::time::SystemTime::now() + Duration::from_secs(10);
        let date = httpdate::fmt_http_date(later);
        assert!(parse_retry_after(&date).is_some_and(|seconds| seconds <= 10));
    }

    #[test]
    fn stats_merge_keeps_the_latest_backoff() {
        let mut first = JiraReadStats {
            requests: 1,
            backoff_until: Some("2026-01-01T00:00:02Z".to_owned()),
            ..JiraReadStats::default()
        };
        let second = JiraReadStats {
            requests: 2,
            retries: 1,
            backoff_until: Some("2026-01-01T00:00:03Z".to_owned()),
            ..JiraReadStats::default()
        };
        first.merge(&second);
        assert_eq!(first.requests, 3);
        assert_eq!(first.retries, 1);
        assert_eq!(first.backoff_until.as_deref(), Some("2026-01-01T00:00:03Z"));
    }
}
