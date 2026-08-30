//! Reading a seat's own refusal as a provider quota state.
//!
//! # Where this sits
//!
//! The usage poller ([`crate::usage`]) asks a vendor's endpoint how much
//! allowance an account has left. That is a *provider report*: structured, and
//! the only source that can move a state back to available without a human.
//! This module is the other half — the *runtime observation*. It only ever
//! learns anything after something was already refused, and what it learns is
//! whatever the vendor happened to say.
//!
//! # What is persisted, and what is not
//!
//! The refusal text is never stored. It arrives as a
//! [`TransientRefusal`], which is not serializable and whose `Debug` is
//! redacted, and it leaves here as three structured facts — a
//! [`ProviderQuotaKind`], an optional reset instant, and a **digest** of the
//! sentence. The digest is what makes a repeat observation recognisable without
//! the store ever holding the words.
//!
//! # Why it is inert by default
//!
//! With no `quota-signals.yml` the signal set is empty and every call here
//! returns `None` before touching the store. A realm that configured nothing
//! behaves exactly as it did before this module existed.

use kontor_accounts::{QuotaSignal, classify};
use kontor_api::state::ApiState;
use kontor_core::id::{AccountProfileId, ContentHash, ProjectId, Timestamp};
use kontor_core::repository::{CapacityRepository, NewProviderQuotaState, ProjectRepository};
use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};
use kontor_runtime::refusal::TransientRefusal;
use tracing::{debug, warn};

/// What one refusal was read as, and whether it changed the Realm's mind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaClassification {
    /// The account the state is about.
    pub account_profile_id: AccountProfileId,
    /// The provider the matching signal named.
    pub provider: String,
    /// The state recorded.
    pub kind: ProviderQuotaKind,
    /// When an exhausted allowance returns, if the vendor stated one.
    pub resets_at: Option<Timestamp>,
    /// Digest of the refusal text. The text itself is never kept.
    pub evidence_hash: ContentHash,
    /// Whether this observation actually wrote a row.
    ///
    /// `false` means the Realm already held exactly this conclusion from a
    /// runtime observation, so the repeat was recognised and dropped.
    pub recorded: bool,
}

/// Classify one transient refusal and record what it proves.
///
/// Returns `None` — writing nothing — when the Realm configured no signals,
/// when the text is not a quota refusal at all (the overwhelmingly common
/// case), when the run is not pinned to an account, or when the matching signal
/// names a provider this account cannot select.
pub fn classify_and_record(
    state: &ApiState,
    project_id: ProjectId,
    account_profile_id: AccountProfileId,
    signals: &[QuotaSignal],
    refusal: &TransientRefusal,
    now: Timestamp,
) -> Option<QuotaClassification> {
    if signals.is_empty() {
        return None;
    }
    let observed = classify(refusal.as_str(), signals)?;

    // A signal set is realm-wide, so a Claude wording could match on a seat
    // pinned to a Codex account. Recording that would block the wrong account
    // on evidence that was never about it.
    let profile = match state
        .with_store(|store| store.get_account_profile(project_id, account_profile_id))
    {
        Ok(Some(profile)) => profile,
        Ok(None) => return None,
        Err(error) => {
            warn!(account = %account_profile_id, detail = %error, "the account profile could not be read");
            return None;
        }
    };
    match kontor_accounts::selectable_providers(&profile) {
        Ok(aliases) if aliases.contains(&observed.provider) => {}
        Ok(_) => {
            debug!(
                account = %account_profile_id,
                provider = %observed.provider,
                "a refusal classified as a provider this account cannot select",
            );
            return None;
        }
        Err(error) => {
            warn!(account = %account_profile_id, detail = %error, "account routing could not be read");
            return None;
        }
    }

    let evidence_hash = refusal.digest();
    let existing = match state.with_store(|store| store.list_provider_quota_states(project_id)) {
        Ok(states) => states.into_iter().find(|entry| {
            entry.account_profile_id == account_profile_id && entry.provider == observed.provider
        }),
        Err(error) => {
            warn!(detail = %error, "provider quota states could not be read");
            return None;
        }
    };

    // Idempotency is per `(account, provider, evidence)`. Three observations of
    // one limit carry one digest, so the second and third recognise themselves
    // and write nothing — while a *different* refusal on the same pair, or the
    // poller's own later report, still gets through.
    let unchanged = existing.as_ref().is_some_and(|row| {
        row.evidence_hash == evidence_hash
            && row.source == ProviderQuotaSource::RuntimeObservation
            && row.state == observed.kind
            && row.resets_at == observed.resets_at
    });
    if unchanged {
        return Some(QuotaClassification {
            account_profile_id,
            provider: observed.provider,
            kind: observed.kind,
            resets_at: observed.resets_at,
            evidence_hash,
            recorded: false,
        });
    }

    let expected_revision = existing
        .as_ref()
        .map_or(kontor_core::id::AggregateRevision::INITIAL, |row| {
            row.revision
        });
    // Windows and credit belong to the poller and to the operator. A refusal
    // says "we were turned away", which is not evidence about how many windows
    // this account has or where its reserve sits, so whatever is stored is
    // carried forward untouched rather than replaced with an empty set.
    let request = NewProviderQuotaState {
        project_id,
        account_profile_id,
        provider: observed.provider.clone(),
        state: observed.kind,
        resets_at: observed.resets_at,
        windows: existing
            .as_ref()
            .map(|row| row.windows.clone())
            .unwrap_or_default(),
        credit: existing.as_ref().and_then(|row| row.credit),
        evidence_hash: evidence_hash.clone(),
        source: ProviderQuotaSource::RuntimeObservation,
        observed_at: now,
        expected_revision,
        updated_at: now,
    };
    match state.with_store(|store| store.set_provider_quota_state(&request)) {
        Ok(_) => Some(QuotaClassification {
            account_profile_id,
            provider: observed.provider,
            kind: observed.kind,
            resets_at: observed.resets_at,
            evidence_hash,
            recorded: true,
        }),
        Err(error) => {
            // A losing race is not a failure of the observation: the row it
            // would have written is the row somebody else just wrote.
            warn!(
                account = %account_profile_id,
                provider = %observed.provider,
                detail = %error,
                "a runtime-observed quota state could not be recorded",
            );
            None
        }
    }
}
