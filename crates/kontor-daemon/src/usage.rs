//! Asking each configured account what its quota is actually doing.
//!
//! # The gap this closes
//!
//! Before this module a Realm learned about a limit in exactly two ways: a run
//! failed and something parsed the refusal, or a person noticed and typed a
//! state in. Both arrive after the damage, and neither can ever say that a
//! window has *reopened*, because nothing refuses when quota is fine. So every
//! `available` row in `provider_quota_states` was a human's assertion, and a
//! Realm whose accounts recovered overnight did not find out until somebody
//! told it.
//!
//! A poll answers the same question from the endpoint the vendor's own client
//! uses. [`kontor_accounts::observe`] owns what a reading *means*; this module
//! owns getting one and writing it down.
//!
//! # Why this is not a command
//!
//! Recording a poll through the command path would mint a receipt every tick —
//! hundreds a day, each saying nothing happened. A command is something an actor
//! asked for; a poll is evidence Kontor went and collected. The row it writes
//! names [`ProviderQuotaSource::ProviderReport`] as its authority, which is
//! exactly how a later reader tells it apart from the operator assertion sitting
//! in the same table.
//!
//! # The credential seam
//!
//! [`ProviderHomes`] is the one place in the daemon where an approved alias
//! becomes a real directory. It is scanned once at startup from a directory only
//! the operator can write, and an alias is never joined onto a path — the
//! directory listing is the source of truth, so a profile naming
//! `../../.ssh` matches nothing rather than escaping anywhere.
//!
//! The approved path is also the input to Claude Code's scoped macOS Keychain
//! service name. Kontor derives that name and reads only that exact entry; the
//! token is never cached, logged or persisted.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kontor_accounts::{
    UsageFailure, UsageReading, observe, read_chatgpt_usage, read_chatgpt_usage_strict,
    read_claude_usage, read_claude_usage_strict,
};
use kontor_api::state::ApiState;
use kontor_core::id::{ContentHash, CredentialAlias, IdempotencyKey, ProviderUsageObservationId};
use kontor_core::repository::{
    AccountProfile, CapacityRepository, CredentialReferenceKind, NewProviderQuotaState,
    NewProviderUsageObservation, ProjectRepository, ProviderUsageObservation, RepositoryError,
};
use kontor_core::spec::ProviderQuotaSource;
use secrecy::{ExposeSecret, SecretString};
use tracing::{debug, info, warn};

mod backoff;

/// The directory inside a state root that holds one subdirectory per account.
pub const PROVIDER_HOMES_DIR: &str = "provider-homes";

/// The credential file a ChatGPT-authenticated home keeps its tokens in.
const CHATGPT_AUTH_FILE: &str = "auth.json";

/// The credential file a Claude-authenticated home keeps its tokens in.
///
/// Older Claude Code builds wrote this inside a custom home. Current macOS
/// builds use a config-home-scoped keychain service instead, so the reader tries
/// this file first and the scoped keychain entry second.
const CLAUDE_AUTH_FILE: &str = ".credentials.json";

/// The service prefix Claude Code 2.1 uses for config-home-scoped credentials.
///
/// The eight-character suffix is the first eight hex characters of the
/// SHA-256 digest of `CLAUDE_CONFIG_DIR`. The unsuffixed service belongs to the
/// default home; custom homes use the suffix, which is what lets two logins
/// coexist in one macOS keychain.
#[cfg(any(target_os = "macos", test))]
const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// The beta the Claude OAuth usage endpoint requires. Omitting it is a 4xx.
const CLAUDE_OAUTH_BETA: &str = "oauth-2025-04-20";

/// One vendor's usage API, and how to authenticate against it.
///
/// A closed enum rather than configuration, deliberately: an operator who could
/// point an endpoint somewhere else could point it at a host that logs the
/// bearer token it is handed. Adding a vendor is a code change that gets read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderApi {
    /// Codex, authenticated by a ChatGPT account.
    Codex,
    /// Claude Code, authenticated by a claude.ai account.
    Claude,
}

impl ProviderApi {
    /// Every vendor this build can poll, in the order a home is probed.
    const ALL: [Self; 2] = [Self::Codex, Self::Claude];

    /// The provider name, spelled as the model catalog spells it.
    const fn provider(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    /// Resolve an exact selectable route to its closed vendor family.
    fn for_provider(provider: &str) -> Option<Self> {
        match crate::applications::provider_family(provider) {
            "codex" => Some(Self::Codex),
            "claude" => Some(Self::Claude),
            _ => None,
        }
    }

    /// The credential file inside a home, and the path to the token within it.
    const fn credential(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Codex => (CHATGPT_AUTH_FILE, &["tokens", "access_token"]),
            Self::Claude => (CLAUDE_AUTH_FILE, &["claudeAiOauth", "accessToken"]),
        }
    }

    /// Where this account's live quota is published.
    const fn endpoint(self) -> &'static str {
        match self {
            Self::Codex => "https://chatgpt.com/backend-api/wham/usage",
            Self::Claude => "https://api.anthropic.com/api/oauth/usage",
        }
    }

    /// Read a successful body into a reading.
    fn read(self, body: &[u8]) -> Result<UsageReading, UsageFailure> {
        match self {
            Self::Codex => read_chatgpt_usage(self.provider(), body),
            Self::Claude => read_claude_usage(self.provider(), body),
        }
    }

    /// Read the provider report as admission evidence, where an absent field
    /// cannot be defaulted into fresh headroom.
    fn read_strict(self, body: &[u8]) -> Result<UsageReading, UsageFailure> {
        match self {
            Self::Codex => read_chatgpt_usage_strict(self.provider(), body),
            Self::Claude => read_claude_usage_strict(self.provider(), body),
        }
    }
}

/// How long a single poll may take before it is abandoned.
const POLL_TIMEOUT: Duration = Duration::from_secs(20);

/// Conservative retry delay when a throttled endpoint sends no usable header.
const DEFAULT_THROTTLE_SECONDS: u64 = 300;

/// Refresh within the configured admission window without weakening that window.
fn poll_interval(evidence_window_seconds: i64) -> Duration {
    Duration::from_millis(
        u64::try_from(evidence_window_seconds)
            .unwrap_or(1)
            .saturating_mul(500)
            .clamp(100, 30_000),
    )
}

/// Both Retry-After delta-seconds and its HTTP-date form are provider retry instructions.
fn retry_after_seconds(headers: &reqwest::header::HeaderMap, now: std::time::SystemTime) -> u64 {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.trim().parse::<u64>().ok().or_else(|| {
                httpdate::parse_http_date(value).ok().map(|date| {
                    date.duration_since(now)
                        .unwrap_or_default()
                        .as_secs()
                        .saturating_add(1)
                })
            })
        })
        .unwrap_or(DEFAULT_THROTTLE_SECONDS)
        .max(1)
}

/// Every approved credential alias, and the directory it actually names.
///
/// `Debug` prints the aliases and never the paths, for the same reason
/// [`kontor_accounts::ResolverPolicy`]'s does: a home directory's path is a fact
/// about a machine's layout, and this value is formatted into logs.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProviderHomes {
    homes: BTreeMap<CredentialAlias, PathBuf>,
}

impl fmt::Debug for ProviderHomes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderHomes")
            .field("aliases", &self.homes.keys().collect::<Vec<_>>())
            .field("paths", &"<redacted>")
            .finish()
    }
}

impl ProviderHomes {
    /// Read every subdirectory of `<state_root>/provider-homes` as one approval.
    ///
    /// A missing or unreadable directory is an empty set, not a failure: a Realm
    /// that has never registered an account is correctly configured, and a
    /// daemon that refused to start over it would be unserviceable for a feature
    /// nobody had asked for yet.
    ///
    /// Each path is canonicalized here, once. Nothing downstream joins anything
    /// onto it.
    #[must_use]
    pub fn discover(state_root: &Path) -> Self {
        let root = state_root.join(PROVIDER_HOMES_DIR);
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Self::default();
        };
        let mut homes = BTreeMap::new();
        for entry in entries.flatten() {
            let Ok(alias) = entry
                .file_name()
                .to_str()
                .ok_or(())
                .and_then(|name| CredentialAlias::parse(name).map_err(|_| ()))
            else {
                continue;
            };
            let Ok(path) = std::fs::canonicalize(entry.path()) else {
                continue;
            };
            if path.is_dir() {
                homes.insert(alias, path);
            }
        }
        Self { homes }
    }

    /// The directory one alias names, when it is approved.
    #[must_use]
    pub fn get(&self, alias: &CredentialAlias) -> Option<&Path> {
        self.homes.get(alias).map(PathBuf::as_path)
    }

    /// Whether nothing at all is approved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.homes.is_empty()
    }

    /// How many aliases are approved.
    #[must_use]
    pub fn len(&self) -> usize {
        self.homes.len()
    }
}

/// The bearer token one vendor's credential file in `home` currently holds.
///
/// Read at the moment of use and dropped straight afterwards — never cached,
/// never returned through a projection, never logged. A home that has been
/// logged out from underneath the Realm therefore reports
/// [`UsageFailure::NoCredential`] on the next poll rather than replaying a token
/// that stopped being valid an hour ago.
///
/// # Errors
/// [`UsageFailure::NoCredential`] for a home with no readable token of this
/// vendor's shape. The underlying I/O and parse errors are dropped rather than
/// wrapped: their `Display` carries the path, and the path is a credential home.
fn file_access_token(home: &Path, api: ProviderApi) -> Result<SecretString, UsageFailure> {
    let (file, path) = api.credential();
    let bytes = std::fs::read(home.join(file)).map_err(|_| UsageFailure::NoCredential)?;
    token_from_document(&bytes, path)
}

fn token_from_document(bytes: &[u8], path: &[&str]) -> Result<SecretString, UsageFailure> {
    let document: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| UsageFailure::NoCredential)?;
    path.iter()
        .try_fold(&document, |node, key| node.get(key))
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(SecretString::from)
        .ok_or(UsageFailure::NoCredential)
}

/// The keychain service Claude Code derives for one custom config home.
#[cfg(any(target_os = "macos", test))]
fn claude_keychain_service(home: &Path) -> Result<String, UsageFailure> {
    let path = home.to_str().ok_or(UsageFailure::NoCredential)?;
    let digest = ContentHash::of(path.as_bytes());
    Ok(format!(
        "{CLAUDE_KEYCHAIN_SERVICE}-{}",
        &digest.as_str()[..8]
    ))
}

#[cfg(target_os = "macos")]
fn claude_keychain_access_token(home: &Path) -> Result<SecretString, UsageFailure> {
    let service = claude_keychain_service(home)?;
    let output = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", &service, "-w"])
        .output()
        .map_err(|_| UsageFailure::NoCredential)?;
    if !output.status.success() {
        return Err(UsageFailure::NoCredential);
    }
    token_from_document(&output.stdout, &["claudeAiOauth", "accessToken"])
}

#[cfg(not(target_os = "macos"))]
fn claude_keychain_access_token(_home: &Path) -> Result<SecretString, UsageFailure> {
    Err(UsageFailure::NoCredential)
}

fn access_token(home: &Path, api: ProviderApi) -> Result<SecretString, UsageFailure> {
    file_access_token(home, api).or_else(|_| match api {
        ProviderApi::Claude => claude_keychain_access_token(home),
        ProviderApi::Codex => Err(UsageFailure::NoCredential),
    })
}

/// Which vendor a credential home belongs to, and its token.
///
/// Duck-typed on the credential document rather than on a naming convention: the
/// poller can only poll what it can authenticate, so "does this home hold a
/// token of shape X" *is* the question "is this an X account". A home called
/// `codex-anything` with no token is correctly skipped, and one called something
/// else entirely is correctly polled. Nothing here trusts the alias.
fn detect(home: &Path) -> Option<(ProviderApi, SecretString)> {
    ProviderApi::ALL
        .into_iter()
        .find_map(|api| access_token(home, api).ok().map(|token| (api, token)))
}

/// Asks each account's provider what its quota is doing.
#[derive(Debug, Clone)]
pub struct UsagePoller {
    homes: ProviderHomes,
    client: reqwest::Client,
    exact_reporter: Option<Arc<dyn ExactProviderUsageReporter>>,
    backoffs: Arc<backoff::ProbeBackoffs>,
}

/// A redacted, closed failure from one explicit provider usage probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderUsageProbeFailure {
    /// This account is not backed by an approved, pollable config home.
    #[error("the account does not support provider usage probes")]
    Unsupported,
    /// The approved home has no usable credential, or the provider rejected it.
    #[error("the provider did not authorize the account usage probe")]
    Unauthorized,
    /// The fixed vendor endpoint could not be reached successfully.
    #[error("the provider usage endpoint could not be reached")]
    Unreachable,
    /// Observation requests were throttled. This never asserts model quota exhaustion.
    #[error("the usage endpoint throttled reads; retry after {retry_after_seconds} seconds")]
    Throttled {
        /// Safe retry delay, derived from Retry-After or the conservative missing-header fallback.
        retry_after_seconds: u64,
    },
}

/// Non-secret reporter seam for one exact configured account/provider route.
///
/// Production leaves this unimplemented and uses the daemon-owned home/token
/// resolver below. Contract tests inject a scripted reporter so they can prove
/// request cardinality and typed failures without possessing any credential.
#[async_trait]
pub trait ExactProviderUsageReporter: fmt::Debug + Send + Sync {
    /// Return one normalized provider report or a closed redacted failure.
    async fn probe(
        &self,
        profile: &AccountProfile,
        provider: &str,
    ) -> Result<UsageReading, ProviderUsageProbeFailure>;
}

impl UsagePoller {
    /// Compose a poller over the accounts registered in a state root.
    #[must_use]
    pub fn discover(state_root: &Path) -> Self {
        Self {
            homes: ProviderHomes::discover(state_root),
            // A failed builder falls back to the default client rather than
            // refusing: a poller that cannot be constructed would take the
            // daemon down over an optional observation.
            client: reqwest::Client::builder()
                .timeout(POLL_TIMEOUT)
                .build()
                .unwrap_or_default(),
            exact_reporter: None,
            backoffs: Arc::new(backoff::ProbeBackoffs::load(state_root)),
        }
    }

    /// The approvals this poller was composed with.
    #[must_use]
    pub const fn homes(&self) -> &ProviderHomes {
        &self.homes
    }

    /// Compose the real home resolver with an injected exact-provider reporter.
    ///
    /// This seam exists for black-box contract tests: the reporter receives
    /// only the persisted non-secret profile and exact provider alias, never a
    /// token, endpoint or resolved home.
    #[doc(hidden)]
    #[must_use]
    pub fn with_exact_reporter(
        state_root: &Path,
        reporter: Arc<dyn ExactProviderUsageReporter>,
    ) -> Self {
        let mut poller = Self::discover(state_root);
        poller.exact_reporter = Some(reporter);
        poller
    }

    /// Ask one account's provider for its current usage.
    ///
    /// Returns `Ok(None)` when the profile is simply not one this build knows
    /// how to poll — a non-config-home reference, an alias with no approved
    /// home, or a home holding no recognised credential. That is a different answer
    /// from a failure, and it must not be recorded as one: writing `unknown`
    /// for every account the poller does not understand would block routes that
    /// were never in trouble.
    ///
    /// # Errors
    /// A [`UsageFailure`] code when the account *is* pollable and the poll did
    /// not produce a reading.
    pub async fn poll(
        &self,
        profile: &AccountProfile,
    ) -> Result<Option<UsageReading>, UsageFailure> {
        match self.probe(profile).await {
            Ok(reading) => Ok(Some(reading)),
            Err(ProviderUsageProbeFailure::Unsupported) => Ok(None),
            Err(ProviderUsageProbeFailure::Unauthorized) => Err(UsageFailure::Unauthorized),
            Err(ProviderUsageProbeFailure::Unreachable) => Err(UsageFailure::Unreachable),
            Err(ProviderUsageProbeFailure::Throttled {
                retry_after_seconds,
            }) => Err(UsageFailure::Throttled {
                retry_after_seconds,
            }),
        }
    }

    /// Ask one exact configured account for a fresh provider report.
    ///
    /// Unlike [`Self::poll`], an account that this build cannot poll is a typed
    /// refusal. This is the operator-facing preflight used before placement.
    /// No credential, home path, endpoint or response body leaves this method.
    pub async fn probe(
        &self,
        profile: &AccountProfile,
    ) -> Result<UsageReading, ProviderUsageProbeFailure> {
        if profile.credential_ref.kind != CredentialReferenceKind::ConfigHome {
            return Err(ProviderUsageProbeFailure::Unsupported);
        }
        let Some(home) = self.homes.get(&profile.credential_ref.alias) else {
            return Err(ProviderUsageProbeFailure::Unsupported);
        };
        let Some((api, token)) = detect(home) else {
            return Err(ProviderUsageProbeFailure::Unauthorized);
        };
        self.guarded_probe(profile, api.provider(), self.request(api, token, false))
            .await
    }

    /// Probe the vendor fixed by an exact configured provider route.
    ///
    /// A home can contain more than one credential document during migration.
    /// Route identity therefore selects the vendor first; token-shape discovery
    /// cannot silently turn a `claude-work` preflight into a Codex observation.
    pub async fn probe_provider(
        &self,
        profile: &AccountProfile,
        provider: &str,
    ) -> Result<UsageReading, ProviderUsageProbeFailure> {
        self.guarded_probe(
            profile,
            provider,
            self.probe_provider_unthrottled(profile, provider),
        )
        .await
    }

    async fn guarded_probe<F>(
        &self,
        profile: &AccountProfile,
        provider: &str,
        request: F,
    ) -> Result<UsageReading, ProviderUsageProbeFailure>
    where
        F: std::future::Future<Output = Result<UsageReading, ProviderUsageProbeFailure>>,
    {
        let key = backoff::ProbeKey::new(profile, provider);
        let latch = self.backoffs.latch(&key);
        let _guard = latch.lock().await;
        if let Some(retry_after_seconds) = self.backoffs.remaining(&key) {
            return Err(ProviderUsageProbeFailure::Throttled {
                retry_after_seconds,
            });
        }
        let result = request.await;
        if let Err(ProviderUsageProbeFailure::Throttled {
            retry_after_seconds,
        }) = &result
            && let Err(error) = self.backoffs.record(key, *retry_after_seconds)
        {
            warn!(kind = ?error.kind(), "usage endpoint backoff retained in memory but could not be persisted");
        }
        result
    }

    async fn probe_provider_unthrottled(
        &self,
        profile: &AccountProfile,
        provider: &str,
    ) -> Result<UsageReading, ProviderUsageProbeFailure> {
        if let Some(reporter) = self.exact_reporter.as_ref() {
            return reporter.probe(profile, provider).await;
        }
        if profile.credential_ref.kind != CredentialReferenceKind::ConfigHome {
            return Err(ProviderUsageProbeFailure::Unsupported);
        }
        let Some(home) = self.homes.get(&profile.credential_ref.alias) else {
            return Err(ProviderUsageProbeFailure::Unsupported);
        };
        let api =
            ProviderApi::for_provider(provider).ok_or(ProviderUsageProbeFailure::Unsupported)?;
        let token = access_token(home, api).map_err(|_| ProviderUsageProbeFailure::Unauthorized)?;
        self.request(api, token, true).await
    }

    async fn request(
        &self,
        api: ProviderApi,
        token: SecretString,
        strict: bool,
    ) -> Result<UsageReading, ProviderUsageProbeFailure> {
        let mut request = self
            .client
            .get(api.endpoint())
            .bearer_auth(token.expose_secret())
            .header(reqwest::header::ACCEPT, "application/json");
        if api == ProviderApi::Claude {
            request = request.header("anthropic-beta", CLAUDE_OAUTH_BETA);
        }
        let response = request
            .send()
            .await
            .map_err(|_| ProviderUsageProbeFailure::Unreachable)?;
        decode_usage_response(api, response, strict).await
    }
}

async fn decode_usage_response(
    api: ProviderApi,
    response: reqwest::Response,
    strict: bool,
) -> Result<UsageReading, ProviderUsageProbeFailure> {
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err(ProviderUsageProbeFailure::Unauthorized);
    }
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderUsageProbeFailure::Throttled {
            retry_after_seconds: retry_after_seconds(
                response.headers(),
                std::time::SystemTime::now(),
            ),
        });
    }
    if !response.status().is_success() {
        return Err(ProviderUsageProbeFailure::Unreachable);
    }
    let body = response
        .bytes()
        .await
        .map_err(|_| ProviderUsageProbeFailure::Unreachable)?;
    if strict {
        api.read_strict(&body)
    } else {
        api.read(&body)
    }
    .map_err(|_| ProviderUsageProbeFailure::Unsupported)
}

/// Poll every enabled account in every project once, and write what came back.
///
/// Returns how many immutable observations were appended. Nothing here refuses: a project that
/// cannot be read, an account that cannot be polled and a row that loses a
/// compare-and-swap are each logged and stepped over, because the alternative is
/// one unreachable account stopping the Realm from observing the others.
pub async fn poll_once(poller: &UsagePoller, state: &ApiState) -> usize {
    if poller.homes().is_empty() && poller.exact_reporter.is_none() {
        return 0;
    }
    let projects = match state.with_store(kontor_store::SqliteStore::list_projects) {
        Ok(projects) => projects,
        Err(error) => {
            warn!(detail = %error, "the project list could not be read for a usage poll");
            return 0;
        }
    };

    let mut jobs = Vec::new();
    for project in projects {
        let profiles = match state
            .with_store(|store| store.list_account_profiles(project.project_id))
        {
            Ok(profiles) => profiles,
            Err(error) => {
                warn!(project = %project.project_id, detail = %error, "account profiles could not be listed");
                continue;
            }
        };
        for profile in profiles.into_iter().filter(|profile| profile.enabled) {
            let aliases = match kontor_accounts::selectable_providers(&profile) {
                Ok(aliases) => aliases,
                Err(error) => {
                    warn!(account = %profile.id, detail = %error, "the account routing document is invalid; its usage poll is skipped");
                    continue;
                }
            };
            if aliases.is_empty() {
                jobs.push((profile, None));
            } else {
                jobs.extend(
                    aliases
                        .into_iter()
                        .map(|provider| (profile.clone(), Some(provider))),
                );
            }
        }
    }
    // One slow endpoint must not age every other account's evidence. Four
    // concurrent bounded reads cover the deployed fleet without an unbounded fan-out.
    use futures::StreamExt;
    futures::stream::iter(jobs)
        .map(|(profile, provider)| async move {
            if let Some(provider) = provider {
                match poller.probe_provider(&profile, &provider).await {
                    Ok(reading) => usize::from(
                        record_exact(state, &profile, &provider, &reading, None, None).is_ok(),
                    ),
                    Err(failure) => {
                        log_poll_failure(&profile, &failure);
                        0
                    }
                }
            } else {
                match poller.poll(&profile).await {
                    Ok(Some(reading)) => record_for_profile(state, &profile, &reading),
                    Ok(None) => 0,
                    Err(failure) => {
                        log_poll_failure(&profile, &failure);
                        0
                    }
                }
            }
        })
        .buffer_unordered(4)
        .fold(0, |written, next| async move { written + next })
        .await
}

fn log_poll_failure(profile: &AccountProfile, failure: &impl fmt::Display) {
    // A pollable account that would not answer is left exactly as it was. It
    // is tempting to write `unknown` here, and wrong: the network being down is
    // a fact about this machine, not about the account's allowance.
    debug!(
        account = %profile.id,
        detail = %failure,
        "an account's usage endpoint did not answer; its quota state is left as it was"
    );
}

/// Record one account reading under the provider aliases that can select it.
///
/// The vendor endpoint names the family (`claude`), while the scheduler routes
/// a concrete account through its alias (`claude-work`). A profile with no
/// declared aliases keeps the family spelling for backwards compatibility.
fn record_for_profile(state: &ApiState, profile: &AccountProfile, reading: &UsageReading) -> usize {
    let aliases = match kontor_accounts::selectable_providers(profile) {
        Ok(aliases) => aliases,
        Err(error) => {
            warn!(account = %profile.id, detail = %error, "the account routing document is invalid; its usage reading is skipped");
            return 0;
        }
    };
    if aliases.is_empty() {
        return usize::from(record(state, profile, reading));
    }
    aliases
        .into_iter()
        .filter(|provider| crate::applications::provider_family(provider) == reading.provider)
        .filter(|provider| {
            let mut routed = reading.clone();
            routed.provider.clone_from(provider);
            record(state, profile, &routed)
        })
        .count()
}

/// Persist one reading as immutable freshness evidence.
///
/// The mutable quota projection changes only when the provider report changes,
/// while every successful call appends a usage observation. This makes an
/// unchanged five-minute poll provable without churning the projection revision.
fn record(state: &ApiState, profile: &AccountProfile, reading: &UsageReading) -> bool {
    record_exact(state, profile, &reading.provider, reading, None, None).is_ok()
}

/// Persist one successful exact-account report and its immutable heartbeat.
///
/// The explicit command key and intent hash are both present for an API probe,
/// or both absent for the background collector. A changed quota projection and
/// the heartbeat commit in one store transaction.
pub fn record_exact(
    state: &ApiState,
    profile: &AccountProfile,
    provider: &str,
    reading: &UsageReading,
    idempotency_key: Option<IdempotencyKey>,
    intent_hash: Option<ContentHash>,
) -> Result<ProviderUsageObservation, RepositoryError> {
    let now = kontor_api::now();
    let mut routed = reading.clone();
    routed.provider = provider.to_owned();
    let observed = observe(&routed);
    let evidence = reading.evidence();

    let existing = match state
        .with_store(|store| store.list_provider_quota_states(profile.project_id))
    {
        Ok(states) => states.into_iter().find(|entry| {
            entry.account_profile_id == profile.id && entry.provider == observed.provider
        }),
        Err(error) => {
            warn!(account = %profile.id, detail = %error, "provider quota states could not be read");
            return Err(error);
        }
    };

    let changed = !existing
        .as_ref()
        .is_some_and(|row| unchanged(row, &evidence));
    let expected_revision = existing
        .as_ref()
        .map_or(kontor_core::id::AggregateRevision::INITIAL, |row| {
            row.revision
        });

    // Observed windows replace the stored set, because a live reading is better
    // evidence than whatever was there. An **empty** reading does not: a
    // document whose only window carried no reset instant, or a vendor that
    // named none at all, must not delete windows an operator recorded by hand.
    // `set_provider_quota_state` replaces the set wholesale, so "observed
    // nothing" and "observed that there is nothing" are indistinguishable at
    // the store — and between those two the non-destructive reading wins.
    //
    // `credit` is never written here at all: a `CreditBalance` carries a
    // *reserve*, and a reserve is an operator's floor rather than anything a
    // vendor reports. Synthesising one would invent policy.
    let windows = if reading.windows.is_empty() {
        existing
            .as_ref()
            .map(|row| row.windows.clone())
            .unwrap_or_default()
    } else {
        reading.windows.clone()
    };
    let quota_state = changed.then(|| NewProviderQuotaState {
        project_id: profile.project_id,
        account_profile_id: profile.id,
        provider: observed.provider.clone(),
        state: observed.kind,
        resets_at: observed.resets_at,
        windows,
        credit: existing.as_ref().and_then(|row| row.credit),
        evidence_hash: evidence.clone(),
        // A structured provider report has no runtime item behind it.
        provenance: None,
        source: ProviderQuotaSource::ProviderReport,
        observed_at: now,
        expected_revision,
        updated_at: now,
    });
    let observation = ProviderUsageObservation {
        id: ProviderUsageObservationId::generate(),
        project_id: profile.project_id,
        account_profile_id: profile.id,
        provider: observed.provider.clone(),
        evidence_hash: evidence,
        state: observed.kind,
        resets_at: observed.resets_at,
        windows: reading.windows.clone(),
        observed_at: now,
    };
    let stored = state.with_store(|store| {
        store.record_provider_usage_observation(&NewProviderUsageObservation {
            observation,
            quota_state,
            idempotency_key,
            intent_hash,
        })
    })?;
    state.signals().appended();
    info!(
        observation = %stored.id,
        account = %profile.id,
        provider = %observed.provider,
        state = observed.kind.as_str(),
        changed,
        "provider usage observed"
    );
    Ok(stored)
}

/// Whether a stored row already says exactly what a new reading says.
fn unchanged(row: &kontor_core::repository::ProviderQuotaState, evidence: &ContentHash) -> bool {
    &row.evidence_hash == evidence && row.source == ProviderQuotaSource::ProviderReport
}

/// Poll on a fixed interval until the daemon is asked to stop.
///
/// The first pass runs immediately rather than after one interval: a daemon that
/// has just come up is exactly when its quota rows are most likely to be stale,
/// because anything that happened while it was down happened unobserved.
pub async fn poll_until_stopped(poller: UsagePoller, state: ApiState) {
    if poller.homes().is_empty() {
        debug!("no credential homes are registered; the usage poller will not run");
        return;
    }
    let interval = poll_interval(state.evidence_window_seconds());
    info!(
        accounts = poller.homes().len(),
        interval_seconds = interval.as_secs(),
        "polling provider usage"
    );
    let mut stops = state.signals().stops();
    let mut ticks = tokio::time::interval(interval);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = ticks.tick() => {}
            // Channel closure also means teardown, never a reason to spin.
            changed = stops.changed() => {
                if changed.is_err() || *stops.borrow() {
                    break;
                }
                continue;
            }
        }
        let written = poll_once(&poller, &state).await;
        if written > 0 {
            debug!(written, "provider usage observations appended from a poll");
        }
    }
    debug!("the usage poller stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Daemon, DaemonConfig};
    use kontor_api::state::RuntimeRegistry;
    use kontor_core::id::{
        AccountProfileId, CanonicalDocument, CredentialAlias, CurrencyCode, ExternalName, Money,
        ProjectId, RuntimeKindKey, parse_utc_timestamp,
    };
    use kontor_core::quota::{CreditBalance, QuotaWindow, QuotaWindowKind};
    use kontor_core::repository::{
        CredentialReference, NewAccountProfile, NewProject, NewProviderQuotaState,
        ProjectRepository,
    };
    use kontor_core::spec::ProviderQuotaKind;

    fn probe_profile(state: &ApiState, provider: &str) -> AccountProfile {
        let project_id = ProjectId::generate();
        let now = kontor_api::now();
        let empty =
            CanonicalDocument::from_value(&serde_json::json!({"schema_version":1})).unwrap();
        state.with_store(|store| {
            store
                .create_project(&NewProject {
                    id: project_id,
                    name: ExternalName::parse(provider).unwrap(),
                    root_path: ExternalName::parse(&format!(
                        "/tmp/usage-probe-fixture-{project_id}"
                    ))
                    .unwrap(),
                    created_at: now,
                })
                .unwrap();
            store
                .create_account_profile(&NewAccountProfile {
                    id: AccountProfileId::generate(),
                    project_id,
                    label: ExternalName::parse(provider).unwrap(),
                    external_account_id: None,
                    harness: RuntimeKindKey::parse("paseo").unwrap(),
                    credential_ref: CredentialReference {
                        kind: CredentialReferenceKind::ConfigHome,
                        alias: CredentialAlias::parse(provider).unwrap(),
                    },
                    environment: empty.clone(),
                    routing: CanonicalDocument::from_value(
                        &serde_json::json!({"schema_version":1,"selectable_providers":[provider]}),
                    )
                    .unwrap(),
                    capability: empty,
                    provider_identity: None,
                    enabled: true,
                    created_at: now,
                })
                .unwrap()
        })
    }

    #[derive(Debug, Default)]
    struct ProbeReporter {
        calls: std::sync::atomic::AtomicUsize,
        throttle_claude: bool,
    }

    #[async_trait]
    impl ExactProviderUsageReporter for ProbeReporter {
        async fn probe(
            &self,
            _: &AccountProfile,
            provider: &str,
        ) -> Result<UsageReading, ProviderUsageProbeFailure> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.throttle_claude && provider.starts_with("claude") {
                return Err(ProviderUsageProbeFailure::Throttled {
                    retry_after_seconds: 600,
                });
            }
            Ok(UsageReading {
                provider: crate::applications::provider_family(provider).to_owned(),
                limit_reached: false,
                windows: Vec::new(),
                credits_exhausted: false,
            })
        }
    }

    #[tokio::test]
    async fn usage_http_429_is_a_retry_instruction_not_quota_exhaustion_or_network_failure() {
        for (header, minimum) in [
            (Some("2312"), 2312),
            (None, DEFAULT_THROTTLE_SECONDS),
            (Some("invalid"), DEFAULT_THROTTLE_SECONDS),
        ] {
            let mut response = axum::http::Response::builder().status(429);
            if let Some(value) = header {
                response = response.header("retry-after", value);
            }
            let response = reqwest::Response::from(
                response
                    .body("{\"error\":{\"type\":\"rate_limit_error\"}}")
                    .unwrap(),
            );
            assert_eq!(
                decode_usage_response(ProviderApi::Claude, response, true).await,
                Err(ProviderUsageProbeFailure::Throttled {
                    retry_after_seconds: minimum
                })
            );
        }
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            httpdate::fmt_http_date(now + Duration::from_secs(45))
                .parse()
                .unwrap(),
        );
        assert!((45..=46).contains(&retry_after_seconds(&headers, now)));
        for seconds in [2, 10, 60, 120] {
            assert!(
                poll_interval(seconds) < Duration::from_secs(u64::try_from(seconds).unwrap()),
                "the collector must refresh before configured positive evidence expires"
            );
        }
    }

    #[tokio::test]
    async fn usage_throttle_survives_restart_and_does_not_starve_other_accounts() {
        let directory = tempfile::tempdir().unwrap();
        let daemon = Daemon::start(
            DaemonConfig::at(directory.path()).with_port(0),
            RuntimeRegistry::new(),
        )
        .unwrap();
        let state = daemon.state();
        let claude = probe_profile(&state, "claude-work");
        let codex = probe_profile(&state, "codex-personal");
        let reporter = Arc::new(ProbeReporter {
            throttle_claude: true,
            ..ProbeReporter::default()
        });
        let poller =
            UsagePoller::with_exact_reporter(directory.path(), Arc::clone(&reporter) as Arc<_>);
        assert_eq!(poll_once(&poller, &state).await, 1);
        assert_eq!(reporter.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(poll_once(&poller, &state).await, 1);
        assert_eq!(
            reporter.calls.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "the throttled endpoint is not called again while the other refreshes"
        );
        assert!(
            state
                .with_store(|store| store.list_provider_quota_states(claude.project_id))
                .unwrap()
                .is_empty()
        );
        assert!(
            state
                .with_store(|store| store.latest_provider_usage_observation(
                    claude.project_id,
                    claude.id,
                    "claude-work"
                ))
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .with_store(|store| store.latest_provider_usage_observation(
                    codex.project_id,
                    codex.id,
                    "codex-personal"
                ))
                .unwrap()
                .is_some()
        );
        drop(poller);
        let restarted =
            UsagePoller::with_exact_reporter(directory.path(), Arc::clone(&reporter) as Arc<_>);
        assert!(matches!(
            restarted.probe_provider(&claude, "claude-work").await,
            Err(ProviderUsageProbeFailure::Throttled {
                retry_after_seconds: 1..=600
            })
        ));
        assert_eq!(
            reporter.calls.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "restart must honor the existing provider retry deadline"
        );
    }

    #[tokio::test]
    async fn usage_throttle_expires_then_requires_a_new_actual_report() {
        #[derive(Debug, Default)]
        struct Once(std::sync::atomic::AtomicUsize);
        #[async_trait]
        impl ExactProviderUsageReporter for Once {
            async fn probe(
                &self,
                _: &AccountProfile,
                _: &str,
            ) -> Result<UsageReading, ProviderUsageProbeFailure> {
                if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Err(ProviderUsageProbeFailure::Throttled {
                        retry_after_seconds: 1,
                    })
                } else {
                    Ok(UsageReading {
                        provider: "claude".to_owned(),
                        limit_reached: false,
                        windows: Vec::new(),
                        credits_exhausted: false,
                    })
                }
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let daemon = Daemon::start(
            DaemonConfig::at(directory.path()).with_port(0),
            RuntimeRegistry::new(),
        )
        .unwrap();
        let profile = probe_profile(&daemon.state(), "claude-work");
        let reporter = Arc::new(Once::default());
        let poller =
            UsagePoller::with_exact_reporter(directory.path(), Arc::clone(&reporter) as Arc<_>);
        assert!(matches!(
            poller.probe_provider(&profile, "claude-work").await,
            Err(ProviderUsageProbeFailure::Throttled { .. })
        ));
        assert!(matches!(
            poller.probe_provider(&profile, "claude-work").await,
            Err(ProviderUsageProbeFailure::Throttled { .. })
        ));
        assert_eq!(reporter.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(poller.probe_provider(&profile, "claude-work").await.is_ok());
        assert_eq!(
            reporter.0.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "elapsed retry time permits a new provider read; it never supplies an invented report"
        );
    }

    #[tokio::test]
    async fn usage_collector_refreshes_unchanged_evidence_automatically_inside_the_window() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(
            directory
                .path()
                .join(PROVIDER_HOMES_DIR)
                .join("codex-personal"),
        )
        .unwrap();
        let mut config = DaemonConfig::at(directory.path()).with_port(0);
        config.evidence_window_seconds = 2;
        let daemon = Daemon::start(config, RuntimeRegistry::new()).unwrap();
        let state = daemon.state();
        let profile = probe_profile(&state, "codex-personal");
        let reporter = Arc::new(ProbeReporter::default());
        let poller = UsagePoller::with_exact_reporter(directory.path(), reporter as Arc<_>);
        let mut appends = state.signals().appends();
        let worker = tokio::spawn(poll_until_stopped(poller, state.clone()));
        let mut first = None;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                appends.changed().await.unwrap();
                let observed = state.with_store(|store| store.latest_provider_usage_observation(profile.project_id,profile.id,"codex-personal")).unwrap().unwrap();
                if let Some((id, at)) = first {
                    if observed.id != id { assert!(observed.observed_at > at); break; }
                } else { first = Some((observed.id,observed.observed_at)); }
            }
        }).await.expect("the automatic collector must refresh within the short configured window, not after five minutes");
        let rows = state
            .with_store(|store| store.list_provider_quota_states(profile.project_id))
            .unwrap();
        assert_eq!(
            rows[0].revision,
            kontor_core::id::AggregateRevision::INITIAL,
            "an unchanged success appends evidence without churning quota state"
        );
        state.signals().stop();
        worker.await.unwrap();
    }

    #[test]
    fn an_absent_provider_homes_directory_approves_nothing() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let homes = ProviderHomes::discover(directory.path());
        assert!(homes.is_empty());
    }

    #[test]
    fn each_subdirectory_is_approved_under_its_own_name_and_files_are_not() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().join(PROVIDER_HOMES_DIR);
        std::fs::create_dir_all(root.join("codex-work")).expect("a home");
        std::fs::create_dir_all(root.join("codex-personal")).expect("a home");
        std::fs::write(root.join("notes.txt"), b"not a home").expect("a stray file");

        let homes = ProviderHomes::discover(directory.path());
        assert_eq!(homes.len(), 2);
        assert!(homes.get(&alias("codex-work")).is_some());
        assert!(homes.get(&alias("codex-personal")).is_some());
    }

    #[test]
    fn an_alias_is_looked_up_and_never_joined_so_traversal_matches_nothing() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().join(PROVIDER_HOMES_DIR);
        std::fs::create_dir_all(root.join("codex-work")).expect("a home");
        let homes = ProviderHomes::discover(directory.path());

        // The domain refuses the shape outright, and even if it did not there is
        // no join for it to escape: `get` is an exact lookup in a map built from
        // a directory listing.
        assert!(CredentialAlias::parse("../../.ssh").is_err());
        assert!(homes.get(&alias("codex-elsewhere")).is_none());
    }

    #[test]
    fn the_approved_paths_are_not_printed_at_any_verbosity() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().join(PROVIDER_HOMES_DIR);
        std::fs::create_dir_all(root.join("codex-work")).expect("a home");
        let homes = ProviderHomes::discover(directory.path());

        let rendered = format!("{homes:?}");
        assert!(
            rendered.contains("codex-work"),
            "the alias is safe to print"
        );
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains(PROVIDER_HOMES_DIR));
    }

    #[test]
    fn a_home_with_no_credential_yields_no_token_for_any_vendor() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        for api in ProviderApi::ALL {
            assert_eq!(
                access_token(directory.path(), api).expect_err("no credential"),
                UsageFailure::NoCredential
            );
        }
        assert!(detect(directory.path()).is_none());

        // A credential file that exists but carries no token is still no token.
        std::fs::write(directory.path().join(CHATGPT_AUTH_FILE), b"{\"tokens\":{}}")
            .expect("an empty credential");
        std::fs::write(
            directory.path().join(CLAUDE_AUTH_FILE),
            b"{\"claudeAiOauth\":{}}",
        )
        .expect("an empty credential");
        assert!(detect(directory.path()).is_none());
    }

    #[test]
    fn a_token_is_read_but_never_rendered() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(
            directory.path().join(CHATGPT_AUTH_FILE),
            br#"{"tokens": {"access_token": "sk-not-a-real-token"}}"#,
        )
        .expect("a credential");

        let token = access_token(directory.path(), ProviderApi::Codex).expect("the token reads");
        assert_eq!(token.expose_secret(), "sk-not-a-real-token");
        assert!(!format!("{token:?}").contains("sk-not-a-real-token"));
    }

    #[test]
    fn a_claude_home_is_recognised_by_its_own_credential_shape() {
        // The nesting differs from Codex's — `claudeAiOauth.accessToken`, not
        // `tokens.access_token` — so a vendor is identified by the shape of the
        // file it wrote rather than by what the directory is called.
        let directory = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(
            directory.path().join(CLAUDE_AUTH_FILE),
            br#"{"claudeAiOauth": {"accessToken": "sk-ant-not-real",
                 "refreshToken": "rt", "subscriptionType": "team"}}"#,
        )
        .expect("a credential");

        let (api, token) = detect(directory.path()).expect("a claude home is detected");
        assert_eq!(api, ProviderApi::Claude);
        assert_eq!(api.provider(), "claude");
        assert_eq!(token.expose_secret(), "sk-ant-not-real");
        assert!(!format!("{token:?}").contains("sk-ant-not-real"));
    }

    #[test]
    fn a_claude_custom_home_selects_its_scoped_keychain_service() {
        assert_eq!(
            claude_keychain_service(Path::new("/tmp/kontor/provider-homes/claude-work"))
                .expect("a UTF-8 path"),
            "Claude Code-credentials-20ebd982"
        );
        assert_eq!(
            claude_keychain_service(Path::new("/tmp/kontor/provider-homes/claude-personal"))
                .expect("a UTF-8 path"),
            "Claude Code-credentials-2c102035"
        );
    }

    #[test]
    fn every_vendor_endpoint_is_https_and_distinct() {
        // A downgraded scheme would put a bearer token on the wire in clear.
        for api in ProviderApi::ALL {
            assert!(
                api.endpoint().starts_with("https://"),
                "{} must be reached over TLS",
                api.provider()
            );
        }
        assert_ne!(
            ProviderApi::Codex.endpoint(),
            ProviderApi::Claude.endpoint(),
            "two vendors sharing one endpoint would send each token to the other"
        );
    }

    #[test]
    fn an_exact_account_alias_selects_only_its_vendor_family() {
        assert_eq!(
            ProviderApi::for_provider("codex-personal"),
            Some(ProviderApi::Codex)
        );
        assert_eq!(
            ProviderApi::for_provider("claude-work"),
            Some(ProviderApi::Claude)
        );
        assert_eq!(ProviderApi::for_provider("opencode"), None);
    }

    #[test]
    fn an_operator_window_set_survives_a_later_poller_record_of_the_same_header() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let daemon = Daemon::start(
            DaemonConfig::at(directory.path()).with_port(0),
            RuntimeRegistry::new(),
        )
        .expect("the realm starts");
        let state = daemon.state();

        let project_id = ProjectId::generate();
        let account_profile_id = AccountProfileId::generate();
        let created_at = parse_utc_timestamp("2026-08-22T09:00:00Z").expect("an instant");
        let windows = vec![
            QuotaWindow {
                kind: QuotaWindowKind::Session,
                resets_at: parse_utc_timestamp("2026-08-22T14:00:00Z").expect("an instant"),
                used_percent: 28,
            },
            QuotaWindow {
                kind: QuotaWindowKind::Weekly,
                resets_at: parse_utc_timestamp("2026-08-23T09:35:00Z").expect("an instant"),
                used_percent: 62,
            },
        ];
        let credit = CreditBalance {
            remaining: Money {
                minor_units: 40_000,
                currency: CurrencyCode::parse("EUR").expect("a currency"),
            },
            reserve: Money {
                minor_units: 10_000,
                currency: CurrencyCode::parse("EUR").expect("a currency"),
            },
        };
        let empty = CanonicalDocument::from_value(&serde_json::json!({ "schema_version": 1 }))
            .expect("a document");
        let profile = state.with_store(|store| {
            store
                .create_project(&NewProject {
                    id: project_id,
                    name: ExternalName::parse("quota").expect("a name"),
                    root_path: ExternalName::parse("/tmp/quota").expect("a path"),
                    created_at,
                })
                .expect("the project is created");
            let profile = store
                .create_account_profile(&NewAccountProfile {
                    id: account_profile_id,
                    project_id,
                    label: ExternalName::parse("work").expect("a name"),
                    external_account_id: None,
                    harness: RuntimeKindKey::parse("paseo").expect("a runtime"),
                    credential_ref: CredentialReference {
                        kind: CredentialReferenceKind::Keychain,
                        alias: CredentialAlias::parse("codex-work").expect("an alias"),
                    },
                    environment: empty.clone(),
                    routing: empty.clone(),
                    capability: empty,
                    provider_identity: None,
                    enabled: true,
                    created_at,
                })
                .expect("the profile is created");
            store
                .set_provider_quota_state(&NewProviderQuotaState {
                    project_id,
                    account_profile_id,
                    provider: "codex".into(),
                    state: ProviderQuotaKind::Available,
                    resets_at: None,
                    windows: windows.clone(),
                    credit: Some(credit),
                    evidence_hash: ContentHash::of(b"the operator said so"),
                    source: ProviderQuotaSource::Operator,
                    observed_at: created_at,
                    expected_revision: kontor_core::id::AggregateRevision::INITIAL,
                    updated_at: created_at,
                    provenance: None,
                })
                .expect("the operator row is recorded");
            profile
        });

        let reading = UsageReading {
            provider: "codex".into(),
            limit_reached: false,
            // The reading names no window — the case that must not delete.
            windows: Vec::new(),
            credits_exhausted: false,
        };
        let written = record(&state, &profile, &reading);
        assert!(written, "an operator row is never skipped as unchanged");

        let restored = state
            .with_store(|store| store.list_provider_quota_states(project_id))
            .expect("the read succeeds")
            .into_iter()
            .find(|row| row.account_profile_id == account_profile_id && row.provider == "codex")
            .expect("the header is still there");
        assert_eq!(
            restored.windows, windows,
            "a poller tick must not DELETE the operator-recorded window set"
        );
        assert_eq!(
            restored.credit,
            Some(credit),
            "a poller tick must not null the operator-recorded credit"
        );
        assert_eq!(restored.source, ProviderQuotaSource::ProviderReport);

        let revision_after_change = restored.revision;
        let heartbeat = record_exact(&state, &profile, "codex", &reading, None, None)
            .expect("an unchanged poll appends a heartbeat");
        let latest = state
            .with_store(|store| {
                store.latest_provider_usage_observation(project_id, account_profile_id, "codex")
            })
            .expect("the latest observation is readable")
            .expect("the heartbeat is durable");
        assert_eq!(latest.id, heartbeat.id);
        let unchanged_projection = state
            .with_store(|store| store.list_provider_quota_states(project_id))
            .expect("the projection is readable")
            .into_iter()
            .find(|row| row.account_profile_id == account_profile_id && row.provider == "codex")
            .expect("the projection remains present");
        assert_eq!(
            unchanged_projection.revision, revision_after_change,
            "an unchanged background heartbeat must not churn the projection"
        );
    }

    #[test]
    fn an_observed_window_set_replaces_the_stored_one() {
        // The other half of the same contract. An empty reading preserves; a
        // reading that actually names windows is better evidence than whatever
        // was there, so it wins — otherwise a stale hand-typed 62% would
        // outrank a live 100% for the rest of the realm's life.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let daemon = Daemon::start(
            DaemonConfig::at(directory.path()).with_port(0),
            RuntimeRegistry::new(),
        )
        .expect("the realm starts");
        let state = daemon.state();

        let project_id = ProjectId::generate();
        let account_profile_id = AccountProfileId::generate();
        let created_at = parse_utc_timestamp("2026-08-22T09:00:00Z").expect("an instant");
        let stale = vec![QuotaWindow {
            kind: QuotaWindowKind::Weekly,
            resets_at: parse_utc_timestamp("2026-08-23T09:35:00Z").expect("an instant"),
            used_percent: 62,
        }];
        let empty = CanonicalDocument::from_value(&serde_json::json!({ "schema_version": 1 }))
            .expect("a document");
        let routing = CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "selectable_providers": ["claude-work"]
        }))
        .expect("a routing document");
        let profile = state.with_store(|store| {
            store
                .create_project(&NewProject {
                    id: project_id,
                    name: ExternalName::parse("quota").expect("a name"),
                    root_path: ExternalName::parse("/tmp/quota").expect("a path"),
                    created_at,
                })
                .expect("the project is created");
            let profile = store
                .create_account_profile(&NewAccountProfile {
                    id: account_profile_id,
                    project_id,
                    label: ExternalName::parse("personal").expect("a name"),
                    external_account_id: None,
                    harness: RuntimeKindKey::parse("paseo").expect("a runtime"),
                    credential_ref: CredentialReference {
                        kind: CredentialReferenceKind::ConfigHome,
                        alias: CredentialAlias::parse("claude-work").expect("an alias"),
                    },
                    environment: empty.clone(),
                    routing,
                    capability: empty,
                    provider_identity: None,
                    enabled: true,
                    created_at,
                })
                .expect("the profile is created");
            store
                .set_provider_quota_state(&NewProviderQuotaState {
                    project_id,
                    account_profile_id,
                    provider: "claude-work".into(),
                    state: ProviderQuotaKind::Available,
                    resets_at: None,
                    windows: stale.clone(),
                    credit: None,
                    evidence_hash: ContentHash::of(b"the operator said so"),
                    source: ProviderQuotaSource::Operator,
                    observed_at: created_at,
                    expected_revision: kontor_core::id::AggregateRevision::INITIAL,
                    updated_at: created_at,
                    provenance: None,
                })
                .expect("the operator row is recorded");
            profile
        });

        let observed = vec![
            QuotaWindow {
                kind: QuotaWindowKind::Session,
                resets_at: parse_utc_timestamp("2026-08-22T18:00:00Z").expect("an instant"),
                used_percent: 7,
            },
            QuotaWindow {
                kind: QuotaWindowKind::Weekly,
                resets_at: parse_utc_timestamp("2026-08-25T09:00:00Z").expect("an instant"),
                used_percent: 100,
            },
        ];
        let written = record_for_profile(
            &state,
            &profile,
            &UsageReading {
                provider: "claude".into(),
                limit_reached: true,
                windows: observed.clone(),
                credits_exhausted: false,
            },
        );
        assert_eq!(written, 1, "a changed reading is recorded once");

        let restored = state
            .with_store(|store| store.list_provider_quota_states(project_id))
            .expect("the read succeeds")
            .into_iter()
            .find(|row| {
                row.account_profile_id == account_profile_id && row.provider == "claude-work"
            })
            .expect("the row is still there");
        assert_eq!(restored.windows, observed, "the live reading wins");
        assert_eq!(restored.source, ProviderQuotaSource::ProviderReport);
        assert_eq!(restored.state, ProviderQuotaKind::Exhausted);
        // The weekly window is the later of the two spent-or-not instants, and
        // it is the one a scheduler must wait for.
        assert_eq!(
            restored.resets_at,
            Some(parse_utc_timestamp("2026-08-25T09:00:00Z").expect("an instant"))
        );
    }

    fn alias(name: &str) -> CredentialAlias {
        CredentialAlias::parse(name).expect("a well-formed alias")
    }
}
