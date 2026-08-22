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

use kontor_accounts::{UsageFailure, UsageReading, observe, read_chatgpt_usage};
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

/// The provider a ChatGPT credential home authenticates, spelled as the model
/// catalog spells it.
const CHATGPT_PROVIDER: &str = "codex";

/// Where an account's live quota is published.
///
/// A constant rather than configuration, and deliberately so: an operator who
/// could point this at another host could point it at one that logs the bearer
/// token it is handed. The endpoint belongs to the same closed decision as
/// [`CHATGPT_AUTH_FILE`] — this build knows how to talk to one usage API, and a
/// second vendor gets a second constant beside this one.
const CHATGPT_USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

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

/// The bearer token a ChatGPT credential home currently holds.
///
/// Read at the moment of use and dropped straight afterwards — never cached,
/// never returned through a projection, never logged. A home that has been
/// logged out from underneath the Realm therefore reports
/// [`UsageFailure::NoCredential`] on the next poll rather than replaying a token
/// that stopped being valid an hour ago.
///
/// # Errors
/// [`UsageFailure::NoCredential`] for a home with no readable ChatGPT token. The
/// underlying I/O and parse errors are dropped rather than wrapped: their
/// `Display` carries the path, and the path is a credential home.
fn access_token(home: &Path) -> Result<SecretString, UsageFailure> {
    let bytes = std::fs::read(home.join(CHATGPT_AUTH_FILE)).map_err(|_| UsageFailure::NoCredential)?;
    let document: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| UsageFailure::NoCredential)?;
    document
        .get("tokens")
        .and_then(|tokens| tokens.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(SecretString::from)
        .ok_or(UsageFailure::NoCredential)
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
        // Duck-typed on the credential home rather than on a naming convention:
        // the poller can only poll what it can authenticate, so "does this home
        // hold a ChatGPT token" is the same question as "is this a Codex
        // account". An alias called `codex-anything` with no token is correctly
        // skipped, and one called something else with a token is correctly
        // polled.
        let token = match access_token(home) {
            Ok(token) => token,
            Err(_) => return Ok(None),
        };
        let response = self
            .client
            .get(CHATGPT_USAGE_ENDPOINT)
            .bearer_auth(token.expose_secret())
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
        read_chatgpt_usage(CHATGPT_PROVIDER, &body).map(Some)
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

    // ponytail: empty windows/credit. This is #82's poller; it still owns the
    // scheduled collection. Headroom routing reads the header row until a later
    // increment maps UsageReading's primary window through this same writer —
    // a second HTTP collector would duplicate it.
    let request = NewProviderQuotaState {
        project_id: profile.project_id,
        account_profile_id: profile.id,
        provider: observed.provider.clone(),
        state: observed.kind,
        resets_at: observed.resets_at,
        windows: Vec::new(),
        credit: None,
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
    fn a_home_with_no_chatgpt_credential_yields_no_token() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        assert_eq!(
            access_token(directory.path()).expect_err("no credential"),
            UsageFailure::NoCredential
        );

        std::fs::write(directory.path().join(CHATGPT_AUTH_FILE), b"{\"tokens\":{}}")
            .expect("an empty credential");
        assert_eq!(
            access_token(directory.path()).expect_err("no credential"),
            UsageFailure::NoCredential
        );
    }

    #[test]
    fn a_token_is_read_but_never_rendered() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(
            directory.path().join(CHATGPT_AUTH_FILE),
            br#"{"tokens": {"access_token": "sk-not-a-real-token"}}"#,
        )
        .expect("a credential");

        let token = access_token(directory.path()).expect("the token reads");
        assert_eq!(token.expose_secret(), "sk-not-a-real-token");
        assert!(!format!("{token:?}").contains("sk-not-a-real-token"));
    }

    fn alias(name: &str) -> CredentialAlias {
        CredentialAlias::parse(name).expect("a well-formed alias")
    }
}
