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
//! This is the same lookup a future account-pinned launch needs: the runtime
//! contract refuses a pin unless the adapter can prove which account a run
//! executes as, and proving it means handing the harness this directory as its
//! configuration home. When that lands it builds a
//! [`kontor_accounts::ResolverPolicy`] from this same map rather than growing a
//! second one.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kontor_accounts::{
    UsageFailure, UsageReading, observe, read_chatgpt_usage, read_claude_usage,
};
use kontor_api::state::ApiState;
use kontor_core::id::{ContentHash, CredentialAlias};
use kontor_core::repository::{
    AccountProfile, CapacityRepository, CredentialReferenceKind, NewProviderQuotaState,
    ProjectRepository,
};
use kontor_core::spec::ProviderQuotaSource;
use secrecy::{ExposeSecret, SecretString};
use tracing::{debug, info, warn};

/// The directory inside a state root that holds one subdirectory per account.
pub const PROVIDER_HOMES_DIR: &str = "provider-homes";

/// The credential file a ChatGPT-authenticated home keeps its tokens in.
const CHATGPT_AUTH_FILE: &str = "auth.json";

/// The credential file a Claude-authenticated home keeps its tokens in.
///
/// Present in a home the operator created; **absent** in the default `~/.claude`,
/// where the token lives in the OS keychain instead. That asymmetry is the whole
/// reason a per-account home is the supported way to register a Claude account
/// here: it puts the credential in a file inside a directory the operator
/// approved, so nothing in this daemon needs to read a secret store.
const CLAUDE_AUTH_FILE: &str = ".credentials.json";

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
}

/// How long a single poll may take before it is abandoned.
const POLL_TIMEOUT: Duration = Duration::from_secs(20);

/// How often every configured account is asked.
///
/// Five minutes is chosen against what the answer is *for*: a route that comes
/// back is worth acting on within one seat's turn, and a plan window that closes
/// is worth noticing before the next batch is placed. It is also cheap — one
/// request per account, and two accounts is the realistic fleet.
///
/// ponytail: one interval for every provider. A per-provider cadence needs a
/// provider taxonomy Kontor does not have; the same note is on
/// [`kontor_accounts::COOLDOWN_SECONDS`], and both become lookups in the
/// account's routing document when one exists.
const POLL_INTERVAL: Duration = Duration::from_secs(300);

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
fn access_token(home: &Path, api: ProviderApi) -> Result<SecretString, UsageFailure> {
    let (file, path) = api.credential();
    let bytes = std::fs::read(home.join(file)).map_err(|_| UsageFailure::NoCredential)?;
    let document: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| UsageFailure::NoCredential)?;
    path.iter()
        .try_fold(&document, |node, key| node.get(key))
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(SecretString::from)
        .ok_or(UsageFailure::NoCredential)
}

/// Which vendor a credential home belongs to, and its token.
///
/// Duck-typed on the credential file rather than on a naming convention: the
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
        }
    }

    /// The approvals this poller was composed with.
    #[must_use]
    pub const fn homes(&self) -> &ProviderHomes {
        &self.homes
    }

    /// Ask one account's provider for its current usage.
    ///
    /// Returns `Ok(None)` when the profile is simply not one this build knows
    /// how to poll — a keychain-backed reference, an alias with no approved
    /// home, or a home holding no ChatGPT credential. That is a different answer
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
        if profile.credential_ref.kind != CredentialReferenceKind::ConfigHome {
            return Ok(None);
        }
        let Some(home) = self.homes.get(&profile.credential_ref.alias) else {
            return Ok(None);
        };
        let Some((api, token)) = detect(home) else {
            return Ok(None);
        };
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
            .map_err(|_| UsageFailure::Unreachable)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(UsageFailure::Unauthorized);
        }
        if !response.status().is_success() {
            return Err(UsageFailure::Unreachable);
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| UsageFailure::Unreachable)?;
        api.read(&body).map(Some)
    }
}

/// Poll every enabled account in every project once, and write what came back.
///
/// Returns how many rows were written. Nothing here refuses: a project that
/// cannot be read, an account that cannot be polled and a row that loses a
/// compare-and-swap are each logged and stepped over, because the alternative is
/// one unreachable account stopping the Realm from observing the others.
pub async fn poll_once(poller: &UsagePoller, state: &ApiState) -> usize {
    if poller.homes().is_empty() {
        return 0;
    }
    let projects = match state.with_store(kontor_store::SqliteStore::list_projects) {
        Ok(projects) => projects,
        Err(error) => {
            warn!(detail = %error, "the project list could not be read for a usage poll");
            return 0;
        }
    };

    let mut written = 0;
    for project in projects {
        let profiles = match state.with_store(|store| store.list_account_profiles(project.project_id)) {
            Ok(profiles) => profiles,
            Err(error) => {
                warn!(project = %project.project_id, detail = %error, "account profiles could not be listed");
                continue;
            }
        };
        for profile in profiles.iter().filter(|profile| profile.enabled) {
            match poller.poll(profile).await {
                Ok(None) => {}
                Ok(Some(reading)) => {
                    if record(state, profile, &reading) {
                        written += 1;
                    }
                }
                // A pollable account that would not answer is left exactly as it
                // was. It is tempting to write `unknown` here, and wrong: the
                // network being down is a fact about this machine, not about the
                // account's allowance, and recording it would block a route that
                // is fine.
                Err(failure) => debug!(
                    account = %profile.id,
                    detail = %failure,
                    "an account's usage endpoint did not answer; its quota state is left as it was"
                ),
            }
        }
    }
    written
}

/// Write one reading into `provider_quota_states`, if it says something new.
///
/// Returns whether a row was written. An unchanged reading is skipped rather
/// than rewritten: the digest is over the numbers, so an identical digest means
/// nothing about the account has moved, and bumping `revision` every five
/// minutes would bury the one change an operator is looking for.
fn record(state: &ApiState, profile: &AccountProfile, reading: &UsageReading) -> bool {
    let now = kontor_api::now();
    let observed = observe(reading);
    let evidence = reading.evidence();

    let existing = match state
        .with_store(|store| store.list_provider_quota_states(profile.project_id))
    {
        Ok(states) => states.into_iter().find(|entry| {
            entry.account_profile_id == profile.id && entry.provider == observed.provider
        }),
        Err(error) => {
            warn!(account = %profile.id, detail = %error, "provider quota states could not be read");
            return false;
        }
    };

    let expected_revision = match &existing {
        Some(row) if unchanged(row, &evidence) => return false,
        Some(row) => row.revision,
        None => kontor_core::id::AggregateRevision::INITIAL,
    };

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
    let request = NewProviderQuotaState {
        project_id: profile.project_id,
        account_profile_id: profile.id,
        provider: observed.provider.clone(),
        state: observed.kind,
        resets_at: observed.resets_at,
        windows,
        credit: existing.as_ref().and_then(|row| row.credit),
        evidence_hash: evidence,
        source: ProviderQuotaSource::ProviderReport,
        observed_at: now,
        expected_revision,
        updated_at: now,
    };
    match state.with_store(|store| store.set_provider_quota_state(&request)) {
        Ok(_) => {
            info!(
                account = %profile.id,
                provider = %observed.provider,
                state = observed.kind.as_str(),
                resets_at = observed.resets_at.map(|instant| instant.to_string()),
                "provider quota observed"
            );
            true
        }
        // A lost compare-and-swap means an operator asserted something while the
        // poll was in flight. Theirs stands until the next tick, which is the
        // right precedence for a five-minute loop racing a person.
        Err(error) => {
            debug!(account = %profile.id, detail = %error, "the quota observation was not applied");
            false
        }
    }
}

/// Whether a stored row already says exactly what a new reading says.
fn unchanged(
    row: &kontor_core::repository::ProviderQuotaState,
    evidence: &ContentHash,
) -> bool {
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
    info!(
        accounts = poller.homes().len(),
        interval_seconds = POLL_INTERVAL.as_secs(),
        "polling provider usage"
    );
    let mut stops = state.signals().stops();
    loop {
        let written = poll_once(&poller, &state).await;
        if written > 0 {
            debug!(written, "provider quota states updated from a usage poll");
        }
        tokio::select! {
            () = tokio::time::sleep(POLL_INTERVAL) => {}
            // `Err` means the sender is gone, which is the daemon being torn
            // down. Treating it as "not stopping" would spin this loop as fast
            // as the channel can report the same closure.
            changed = stops.changed() => {
                if changed.is_err() || *stops.borrow() {
                    break;
                }
            }
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
        AccountProfileId, CanonicalDocument, CredentialAlias, CurrencyCode, ExternalName,
        Money, ProjectId, RuntimeKindKey, parse_utc_timestamp,
    };
    use kontor_core::quota::{CreditBalance, QuotaWindow, QuotaWindowKind};
    use kontor_core::repository::{
        CredentialReference, NewAccountProfile, NewProject, NewProviderQuotaState, ProjectRepository,
    };
    use kontor_core::spec::ProviderQuotaKind;

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
        assert!(rendered.contains("codex-work"), "the alias is safe to print");
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
        std::fs::write(directory.path().join(CLAUDE_AUTH_FILE), b"{\"claudeAiOauth\":{}}")
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

        let token =
            access_token(directory.path(), ProviderApi::Codex).expect("the token reads");
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
                })
                .expect("the operator row is recorded");
            profile
        });

        let written = record(
            &state,
            &profile,
            &UsageReading {
                provider: "codex".into(),
                limit_reached: false,
                // The reading names no window — the case that must not delete.
                windows: Vec::new(),
                credits_exhausted: false,
            },
        );
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
                    provider: "claude".into(),
                    state: ProviderQuotaKind::Available,
                    resets_at: None,
                    windows: stale.clone(),
                    credit: None,
                    evidence_hash: ContentHash::of(b"the operator said so"),
                    source: ProviderQuotaSource::Operator,
                    observed_at: created_at,
                    expected_revision: kontor_core::id::AggregateRevision::INITIAL,
                    updated_at: created_at,
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
        let written = record(
            &state,
            &profile,
            &UsageReading {
                provider: "claude".into(),
                limit_reached: true,
                windows: observed.clone(),
                credits_exhausted: false,
            },
        );
        assert!(written, "a changed reading is recorded");

        let restored = state
            .with_store(|store| store.list_provider_quota_states(project_id))
            .expect("the read succeeds")
            .into_iter()
            .find(|row| row.account_profile_id == account_profile_id && row.provider == "claude")
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
