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
use kontor_core::repository::{
    CapacityRepository, NewProviderQuotaState, NewQuotaObservationProvenance, ProjectRepository,
    RepositoryError,
};
use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};
use kontor_runtime::refusal::TransientRefusal;
use tracing::debug;

/// A conclusion, and the write that would make it durable.
///
/// Split from the write on purpose: the row has to land in the *same*
/// transaction as the observation that proves it, so this produces the request
/// and the caller hands it to `record_observation` rather than writing here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaDecision {
    /// What the refusal was read as.
    pub classification: QuotaClassification,
    /// The row to write, or `None` when the Realm already holds exactly this
    /// conclusion and nothing needs to change.
    pub request: Option<NewProviderQuotaState>,
}

impl QuotaDecision {
    /// Whether the proposal flag and the presence of a request agree.
    ///
    /// They are two spellings of one fact, and a caller serializing the flag
    /// into durable evidence needs them not to drift.
    #[must_use]
    pub const fn proposes_write_matches_request(&self) -> bool {
        self.classification.proposes_write == self.request.is_some()
    }
}

/// Why a refusal could not be turned into a durable conclusion.
///
/// Distinct from "this text is not a quota refusal", which is an ordinary
/// `Ok(None)`. These are failures of the Realm's own storage or configuration,
/// and swallowing them would let a seat keep running on an account whose limit
/// the Realm believes it recorded.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum QuotaObservationError {
    /// An account profile or quota row could not be read or written.
    #[error("the realm's account or quota rows could not be reached")]
    Repository(#[source] RepositoryError),
    /// The account's routing document could not be read.
    #[error("the account's routing document could not be read")]
    Routing(#[source] kontor_core::DomainError),
    /// The row could not be settled within the bounded retry budget.
    ///
    /// Deliberately typed rather than reported as success: the caller asked for
    /// a conclusion to be durable, and after this it is not.
    #[error("a runtime-observed quota state did not settle after {attempts} attempts")]
    Unsettled {
        /// How many writes were attempted.
        attempts: u32,
    },
}

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
    /// Whether this decision *proposes* a write.
    ///
    /// Deliberately not called `recorded`. This is decided before the store is
    /// touched, and the store may still decline to apply it — an out-of-order
    /// or replayed observation is appended as evidence but reduces nothing, and
    /// its quota conclusion is correctly skipped. A field named for a write
    /// that had not happened yet was serialized into immutable raw evidence and
    /// told an operator a row had changed when none had.
    ///
    /// `false` means the Realm already holds exactly this conclusion, so there
    /// is nothing to propose.
    pub proposes_write: bool,
}

/// Classify one transient refusal and record what it proves.
///
/// Returns `None` — writing nothing — when the Realm configured no signals,
/// when the text is not a quota refusal at all (the overwhelmingly common
/// case), when the run is not pinned to an account, or when the matching signal
/// names a provider this account cannot select.
pub fn decide(
    state: &ApiState,
    project_id: ProjectId,
    account_profile_id: AccountProfileId,
    signals: &[QuotaSignal],
    refusal: &TransientRefusal,
    now: Timestamp,
) -> Result<Option<QuotaDecision>, QuotaObservationError> {
    if signals.is_empty() {
        return Ok(None);
    }
    // Eligibility first, classification second. A signal set is realm-wide and
    // a deployment names *exact catalog aliases*, so one vendor's wording
    // appears once per account: `codex-work` and `codex-personal` carry the
    // identical sentence under different aliases.
    //
    // Classifying the whole set and rejecting an ineligible answer afterwards
    // -- which is what this did first -- is wrong twice over. It is inert when
    // no signal happens to name an alias this account can select, and worse, it
    // is *masking*: `classify` returns the first signal whose markers match, so
    // a seat on `codex-personal` matches the `codex-work` entry, gets it thrown
    // away, and never reaches its own identical wording. Filtering to what this
    // account may actually select, in the configured order, makes both
    // impossible: every candidate is eligible by construction, and no
    // ineligible entry can stand in front of an eligible one.
    let profile =
        match state.with_store(|store| store.get_account_profile(project_id, account_profile_id)) {
            Ok(Some(profile)) => profile,
            // A run pointing at an account that no longer exists is a legitimate
            // no-op, not a storage failure.
            Ok(None) => return Ok(None),
            Err(error) => return Err(QuotaObservationError::Repository(error)),
        };
    let selectable = match kontor_accounts::selectable_providers(&profile) {
        Ok(aliases) => aliases,
        Err(error) => return Err(QuotaObservationError::Routing(error)),
    };
    if selectable.is_empty() {
        // An account addressable under no alias cannot own a provider quota
        // row, so there is nothing this refusal could truthfully say about it.
        debug!(
            account = %account_profile_id,
            "the account declares no selectable provider; classification is inert for it",
        );
        return Ok(None);
    }
    let eligible: Vec<QuotaSignal> = signals
        .iter()
        .filter(|signal| selectable.contains(&signal.provider))
        .cloned()
        .collect();
    if eligible.is_empty() {
        debug!(
            account = %account_profile_id,
            "no configured signal names an alias this account may select",
        );
        return Ok(None);
    }
    // The basis for a time-only reset is the provider item's own instant.
    let Some(mut observed) = classify(
        refusal.as_str(),
        &eligible,
        refusal.provenance().observed_at,
    ) else {
        return Ok(None);
    };

    // A refusal we just saw cannot also be already over.
    //
    // `blocks_at` stops blocking once `resets_at` has passed, so recording an
    // allowance whose stated instant is at or before `now` writes a row that is
    // born unblocking: the walk immediately re-admits the account that just
    // refused, the seat refuses again, and the pair spins. That is not a
    // theoretical ordering worry -- a vendor printing a bare local wall clock
    // with the wrong zone, or a clock a few seconds behind, produces it.
    //
    // `Unknown` is the honest reading: a provider refused and this row cannot
    // say when it returns. It blocks, carries no instant, and is exactly the
    // classifier's own visible prompt to fix the signal.
    if observed.kind == ProviderQuotaKind::Exhausted
        && observed.resets_at.is_some_and(|reset| reset <= now)
    {
        debug!(
            account = %account_profile_id,
            provider = %observed.provider,
            "a stated reset is not in the future; recording an unknown block instead",
        );
        observed.kind = ProviderQuotaKind::Unknown;
        observed.resets_at = None;
    }

    let evidence_hash = refusal.digest();
    let existing = current_row(state, project_id, account_profile_id, &observed.provider)?;
    if existing
        .as_ref()
        .is_some_and(|row| already_records(row, &observed, &evidence_hash))
    {
        // The Realm already holds exactly this conclusion. Nothing to write, and
        // `recorded` says so: the effect happened once and this is not it.
        return Ok(Some(QuotaDecision {
            classification: QuotaClassification {
                account_profile_id,
                provider: observed.provider,
                kind: observed.kind,
                resets_at: observed.resets_at,
                evidence_hash,
                proposes_write: false,
            },
            request: None,
        }));
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
        // The record that lets this row be re-judged later: which fingerprint
        // fired, at which version, from which definition, and the exact item
        // that carried it. Modelled scalars and a digest only -- the vendor's
        // sentence is never part of it, which is what lets the record be
        // carried through readback and export unredacted.
        provenance: observed.signal.as_ref().map(|matched| {
            let where_from = refusal.provenance();
            NewQuotaObservationProvenance {
                id: kontor_core::id::QuotaObservationProvenanceId::generate(),
                project_id,
                account_profile_id,
                provider: observed.provider.clone(),
                signal_id: matched.id.clone(),
                signal_version: matched.version,
                signal_definition_hash: matched.definition_hash.clone(),
                agent_run_id: where_from.agent_run_id,
                runtime_binding_id: where_from.runtime_binding_id,
                native_id: where_from.native_id.clone(),
                binding_generation: where_from.binding_generation,
                item_epoch: where_from.position.epoch,
                item_seq_start: where_from.position.sequence,
                item_seq_end: where_from.sequence_end,
                source_sequences: where_from.source_sequences.clone(),
                item_kind: where_from.item_type.clone(),
                item_observed_at: where_from.observed_at,
                decision_basis: kontor_core::spec::QuotaDecisionBasis::RuntimeRefusal,
                decided_state: observed.kind,
                parsed_resets_at: observed.resets_at,
                reset_zone: eligible
                    .iter()
                    .find(|signal| signal.id == matched.id)
                    .and_then(|signal| signal.reset_zone.clone()),
                evidence_digest: evidence_hash.clone(),
                recorded_at: now,
            }
        }),
        source: ProviderQuotaSource::RuntimeObservation,
        // The instant the *item* was emitted, not the instant we looked. A
        // probe runs on inspection, so `now` would make a refusal captured
        // hours ago look freshly observed and let it overwrite a newer poller
        // report -- which is exactly what the store's recency rule exists to
        // stop, and it cannot stop it if the caller lies about when.
        observed_at: refusal.provenance().observed_at,
        expected_revision,
        updated_at: now,
    };
    Ok(Some(QuotaDecision {
        classification: QuotaClassification {
            account_profile_id,
            provider: observed.provider,
            kind: observed.kind,
            resets_at: observed.resets_at,
            evidence_hash,
            proposes_write: true,
        },
        request: Some(request),
    }))
}

/// The stored row for one `(account, provider)` pair, if there is one.
fn current_row(
    state: &ApiState,
    project_id: ProjectId,
    account_profile_id: AccountProfileId,
    provider: &str,
) -> Result<Option<kontor_core::repository::ProviderQuotaState>, QuotaObservationError> {
    Ok(state
        .with_store(|store| store.list_provider_quota_states(project_id))
        .map_err(QuotaObservationError::Repository)?
        .into_iter()
        .find(|entry| entry.account_profile_id == account_profile_id && entry.provider == provider))
}

/// Whether a stored row already records exactly this conclusion.
///
/// Every field that distinguishes one runtime observation from another is
/// compared. A row that merely *blocks* the same pair is not the same fact: the
/// poller's `available`, an operator override, or a different refusal all
/// differ here, and treating any of them as "already recorded" would drop a
/// conclusion on the floor and report success.
fn already_records(
    row: &kontor_core::repository::ProviderQuotaState,
    observed: &kontor_accounts::ObservedQuota,
    evidence_hash: &ContentHash,
) -> bool {
    row.source == ProviderQuotaSource::RuntimeObservation
        && row.state == observed.kind
        && row.resets_at == observed.resets_at
        && &row.evidence_hash == evidence_hash
}
