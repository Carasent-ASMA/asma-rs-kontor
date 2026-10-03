//! One error envelope, one closed vocabulary of codes, and nothing else on the
//! wire.
//!
//! Every refusal this API can produce is an [`ApiError`]. It carries the Realm it
//! was refused in, a stable machine code and a *static* rule — never the request
//! body, never a token, never a runtime URL, never a line of session content. A
//! caller can therefore log the whole envelope, and a test can assert on it,
//! without either of them becoming a place secrets accumulate.
//!
//! Structured detail is limited to a position, a revision, or a validated
//! foreign correlation id inside a closed native-refusal variant — values the
//! caller already had or needs in order to correct the refusal.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kontor_core::id::{AggregateRevision, EventCursor, ExternalId, RealmId};
use kontor_core::realm::RealmCursor;
use kontor_core::repository::RepositoryError;
use kontor_core::{DomainError, closed_enum};
use kontor_runtime::adapter::RuntimeError;
use serde::Serialize;
use tracing::warn;
use utoipa::ToSchema;

/// A closed, non-secret refusal emitted by a native session runtime.
///
/// Arbitrary runtime text never enters this type. The only carried value is a
/// validated foreign identifier needed to correct placement configuration and
/// correlate the refusal with the runtime's own registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NativeRuntimeRefusal {
    /// Paseo refused creation because the supplied caller agent was absent.
    CallerAgentNotFound {
        /// The exact native caller that was refused.
        #[schema(value_type = String)]
        caller_agent_id: ExternalId,
    },
}

closed_enum! {
    /// The stable machine-readable reason a request was refused.
    ///
    /// A client branches on this and on nothing else. The accompanying `rule` is
    /// for a human reading a log.
    ApiErrorCode, "ApiErrorCode" {
        /// No usable credential was presented.
        Unauthenticated => "unauthenticated",
        /// The credential is valid but does not carry the required authority.
        Forbidden => "forbidden",
        /// A cursor, envelope or identifier belongs to another Realm.
        RealmMismatch => "realm_mismatch",
        /// The caller's expected aggregate revision is not the current one.
        RevisionConflict => "revision_conflict",
        /// An idempotency key was reused for a different command.
        IdempotencyConflict => "idempotency_conflict",
        /// An `:ensure` named a label that is taken by something it does not
        /// describe.
        ///
        /// Distinct from `revision_conflict`, which it used to be reported as,
        /// and the difference is not cosmetic: `revision_conflict` tells a
        /// caller to re-read and retry with a fresher revision, and an ensure
        /// takes no revision argument, so there is no retry that satisfies it.
        /// A caller that follows the advice loops. What actually clears this is
        /// amending the entity that holds the name, or choosing another name.
        EnsureMismatch => "ensure_mismatch",
        /// The binding's frozen capability set does not cover this operation.
        UnsupportedCapability => "unsupported_capability",
        /// An identity-preserving native-root rename is required but the
        /// configured runtime cannot prove it can perform one.
        RenamePending => "rename_pending",
        /// The binding no longer names a session this runtime will act on.
        StaleBinding => "stale_binding",
        /// The requested position is outside the retained control-plane history.
        ResnapshotRequired => "resnapshot_required",
        /// The session's content must be read again from the runtime.
        TimelineRefetchRequired => "timeline_refetch_required",
        /// Startup reconciliation has not finished, so scheduling is still shut.
        ReconciliationPending => "reconciliation_pending",
        /// A configured concurrency ceiling is spent. The request was well
        /// formed and the presented state was current; there is simply no room
        /// right now, and there will be when other work finishes.
        ///
        /// Distinct from `revision_conflict` on purpose. A client that cannot
        /// tell the two apart re-reads and retries against a fresh revision,
        /// which is exactly the one thing that never clears this.
        CapacityExhausted => "capacity_exhausted",
        /// A declared role slot was never bound to a session, and nothing has
        /// excused it.
        ///
        /// An explicit refusal, never a persisted negative disposition: the slot
        /// stays outstanding until it is either bound and settled, or waived
        /// under the frozen template's own policy. Recording "could not" as an
        /// accounting source is exactly what this code exists to avoid.
        RoleSlotUnbound => "role_slot_unbound",
        /// A durable handoff exists but its turn disposition has not yet been
        /// recorded, so runtime settlement must not terminalize the run.
        HandoffUnsettled => "handoff_unsettled",
        /// The seat's placement in the session topology is not resolvable, so
        /// nothing was started.
        ///
        /// Every case is a disagreement about *where* work belongs: no node for
        /// the task, a node whose kind hosts no session, a parent with no bound
        /// container, a working directory that is not the bound one, or a slot
        /// that already holds a live seat. None of them is repaired here —
        /// placing the seat anyway is how a team's roles end up split across two
        /// containers, and the repair is a decision a human makes.
        PlacementBlocked => "placement_blocked",
        /// A dependency could not be reached. A fact about the channel only.
        ///
        /// Load bearing: this code's action tells a caller *nothing was
        /// changed*. It may therefore only be used when nothing was — that is,
        /// when the failure happened before any effect was attempted. A channel
        /// that died *after* a write was put on the wire is
        /// [`Self::DeliveryUnconfirmed`], not this.
        Unavailable => "unavailable",
        /// A settlement's canonical proof scan could not be completed, so the
        /// proof was neither accepted nor rejected.
        ///
        /// Distinct from [`Self::Unavailable`], which blames the channel, and
        /// from [`Self::RevisionConflict`], which blames the caller's tuple.
        /// Both were wrong here: the runtime answers bounded reads and reports
        /// itself reachable, and the tuple may be perfectly valid — the daemon
        /// simply could not read far enough to decide. Saying "the runtime
        /// could not be reached" sends an operator to look at a healthy plane,
        /// and saying the tuple is not the current turn accuses them of a
        /// forgery they did not commit.
        ///
        /// Nothing was changed: the scan runs entirely before any write.
        ProofScanIncomplete => "proof_scan_incomplete",
        /// A write was put on the wire and the channel failed before its
        /// outcome could be confirmed. It may well have landed.
        ///
        /// Separate from [`Self::Unavailable`] for one reason, and it is not a
        /// shade of meaning: the two demand opposite actions. `unavailable`
        /// says nothing happened, so retry. This says something may have
        /// happened, so *read before you act* — and never resend under a fresh
        /// identifier, because that is how one instruction becomes two native
        /// turns in a seat's transcript.
        DeliveryUnconfirmed => "delivery_unconfirmed",
        /// The provider rejected the exact configured account credential while
        /// answering its fixed usage endpoint.
        ProviderUnauthorized => "provider_unauthorized",
        /// The fixed provider usage endpoint could not be reached successfully.
        ProviderUnreachable => "provider_unreachable",
        /// The usage endpoint throttled observation requests, not model execution.
        ProviderUsageThrottled => "provider_usage_throttled",
        /// The exact account or successful provider response is not supported
        /// by this build's closed usage-reader set.
        ProviderUnsupported => "provider_unsupported",
        /// The addressed thing does not exist in this Realm.
        NotFound => "not_found",
        /// The request itself is malformed: a missing header, an unparseable
        /// identifier, two headers that contradict each other.
        ///
        /// Not in the architect's twelve, and deliberately added: without it a
        /// missing `Idempotency-Key` would have to be reported as one of the
        /// others, and every one of those would be a lie about what happened.
        InvalidRequest => "invalid_request",
    }
}

impl ApiErrorCode {
    /// The status this code is always reported with.
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::RealmMismatch
            | Self::RevisionConflict
            | Self::IdempotencyConflict
            | Self::EnsureMismatch
            | Self::StaleBinding
            | Self::TimelineRefetchRequired => StatusCode::CONFLICT,
            // The request is well formed and understood; this runtime simply
            // cannot do it. That is not a server defect, so it is not a 5xx.
            Self::UnsupportedCapability | Self::RenamePending | Self::HandoffUnsettled => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            // The position the caller wants is genuinely gone.
            Self::ResnapshotRequired => StatusCode::GONE,
            // The request is well formed and the state it presented is current;
            // what is missing is an accounting the caller must supply or excuse.
            // Same reasoning as `UnsupportedCapability`: not a server defect.
            Self::RoleSlotUnbound => StatusCode::UNPROCESSABLE_ENTITY,
            // Understood, well formed, and refused on the state of the world.
            // A retry against a fresh revision never clears it; a placement
            // decision does.
            Self::PlacementBlocked => StatusCode::CONFLICT,
            // Nothing is wrong with the request or the state it presented, so a
            // 4xx that blames either would misdirect. "Too many requests" is
            // what a spent ceiling is, and it is the status a client already
            // knows to back off and retry on.
            Self::CapacityExhausted | Self::ProviderUsageThrottled => StatusCode::TOO_MANY_REQUESTS,
            Self::ReconciliationPending
            | Self::Unavailable
            | Self::DeliveryUnconfirmed
            | Self::ProofScanIncomplete
            | Self::ProviderUnreachable => StatusCode::SERVICE_UNAVAILABLE,
            Self::ProviderUnauthorized => StatusCode::BAD_GATEWAY,
            Self::ProviderUnsupported => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    /// What a caller holding only this code can try.
    ///
    /// A floor, not the answer: a refusal that knows more about its own cause
    /// says so through [`ApiError::advising`]. What this rules out is a refusal
    /// with *no* corrective line at all, which is the shape an operator can do
    /// nothing with.
    #[must_use]
    pub const fn default_action(self) -> &'static str {
        match self {
            Self::Unauthenticated => "present a credential for this realm",
            Self::Forbidden => "present a credential carrying the tier this operation requires",
            Self::RealmMismatch => "re-read the value from this realm and retry with that one",
            Self::RevisionConflict => {
                "re-read the aggregate and retry with the revision it reports"
            }
            Self::IdempotencyConflict => {
                "use a fresh idempotency key, or retry the original request unchanged"
            }
            Self::EnsureMismatch => {
                "amend the entity that already holds the name, or ensure under a name nothing holds"
            }
            Self::UnsupportedCapability => {
                "read the runtime's capabilities and use an operation it declares"
            }
            Self::RenamePending => {
                "upgrade or enable the runtime's identity-preserving project rename, then retry the preview"
            }
            Self::StaleBinding => "settle the run to learn what its runtime now reports",
            Self::ResnapshotRequired => "read a fresh snapshot and resume from its cursor",
            Self::TimelineRefetchRequired => "read the session timeline again from the runtime",
            Self::ReconciliationPending => "wait for startup reconciliation to finish, then retry",
            Self::CapacityExhausted => {
                "retry when work in flight finishes; nothing was refused about the request itself"
            }
            Self::RoleSlotUnbound => {
                "bind the outstanding role slot, or waive it under the frozen template's policy"
            }
            Self::HandoffUnsettled => "settle the outstanding turn before terminalizing the run",
            Self::PlacementBlocked => "resolve where the work belongs in the topology, then retry",
            Self::Unavailable => "retry once the dependency answers; nothing was changed",
            Self::ProofScanIncomplete => {
                "settle again naming the same turn once the session's canonical history is readable to its end; the proof was neither accepted nor rejected and nothing was changed"
            }
            Self::DeliveryUnconfirmed => {
                "read this session's timeline for the exact idempotency key to learn the outcome, then replay that same key; never resend under a new one"
            }
            Self::ProviderUnauthorized => {
                "reauthenticate the exact configured provider account, then retry"
            }
            Self::ProviderUnreachable => {
                "retry after the provider usage endpoint is reachable; nothing was changed"
            }
            Self::ProviderUsageThrottled => {
                "honor retry_after_seconds before probing again; this is not evidence that model quota is exhausted"
            }
            Self::ProviderUnsupported => {
                "use an enabled config-home account and provider response supported by this build"
            }
            Self::NotFound => "check the identifier against a read of this realm",
            Self::InvalidRequest => "correct the request and send it again",
        }
    }
}

/// What to advise a caller whose transition was refused.
///
/// `from` and `to` are `&'static str` state names, so the advice can quote them
/// — but the returned action is itself `&'static str`, which means it cannot be
/// built by formatting. Two fixed lines rather than one generated one: the
/// common mistake is asking for the state the aggregate is already in, and
/// telling someone to "move it to X" when it is already at X is the kind of
/// advice that wastes an afternoon.
const fn illegal_transition_action(from: &'static str, to: &'static str) -> &'static str {
    if const_str_eq(from, to) {
        "the aggregate is already in the state that was asked for; no transition is needed"
    } else {
        "read the aggregate's current state and choose a transition it accepts from there"
    }
}

/// `str` equality in a `const fn`, which `==` is not.
const fn const_str_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// The JSON body every refusal is reported with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ApiErrorBody {
    /// Safe delay before retrying a throttled usage read; quota projections were not changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
    /// The Realm the request was refused in.
    #[schema(value_type = String)]
    pub realm_id: RealmId,
    /// The stable machine-readable code.
    #[schema(value_type = String)]
    pub code: ApiErrorCode,
    /// A static description of the rule that refused. Never a stored value.
    pub rule: &'static str,
    /// The revision the aggregate actually stands at, for a revision conflict.
    #[schema(value_type = Option<u64>)]
    pub current_revision: Option<AggregateRevision>,
    /// The oldest position still retained, for a resnapshot.
    #[schema(value_type = Option<i64>)]
    pub oldest_retained_cursor: Option<EventCursor>,
    /// The newest position allocated, for a resnapshot.
    #[schema(value_type = Option<i64>)]
    pub newest_cursor: Option<EventCursor>,
    /// Which type, field or state machine refused, when the refusal names one.
    ///
    /// Always a `&'static str` written in this workspace — a type name, a field
    /// name or an operation name — so it can never be a stored value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<&'static str>,
    /// The structural path of the offending node, when the refusal has one.
    ///
    /// A path and never a value: `steps[2].instruction`, not what was in it.
    /// This is the field that turns "something in your document was refused"
    /// into something a caller can actually go and look at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// A safe, closed native-runtime refusal, when the runtime answered with
    /// one. Never an arbitrary stderr or daemon-log message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_runtime_refusal: Option<NativeRuntimeRefusal>,
    /// What the caller can do about it, in one line.
    ///
    /// Corrective rather than descriptive: a refusal that only restates itself
    /// leaves an operator with nothing to try.
    pub action: &'static str,
}

/// Everything this API can refuse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {rule}")]
pub struct ApiError {
    /// The Realm the refusal happened in.
    pub realm_id: RealmId,
    /// The stable code.
    pub code: ApiErrorCode,
    /// The static rule.
    pub rule: &'static str,
    /// The current revision, when the caller is owed one.
    pub current_revision: Option<AggregateRevision>,
    /// The retained window, when the caller must resnapshot.
    pub retained: Option<(EventCursor, EventCursor)>,
    /// Where the refusal happened, when it can say.
    ///
    /// Boxed because it is the uncommon case and [`ApiError`] is the `Err` of
    /// almost every function in this crate: two more inline fields push the
    /// whole `Result` past the size a large-error lint accepts, and paying that
    /// on every successful call to describe the rare failure is the wrong
    /// trade.
    pub diagnostic: Option<Box<ErrorDiagnostic>>,
    /// The exact closed native refusal, when one was recognized.
    pub native_runtime_refusal: Option<Box<NativeRuntimeRefusal>>,
    /// The one-line corrective action.
    pub action: &'static str,
}

/// Where a refusal happened, in structural terms only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDiagnostic {
    /// Delay for a provider-throttled observation, never a quota reset assertion.
    pub retry_after_seconds: Option<u64>,
    /// The type, field or state machine that refused.
    pub subject: Option<&'static str>,
    /// The structural path of the offending node. Never its value.
    pub at: Option<String>,
}

impl ApiError {
    /// Build a refusal.
    #[must_use]
    pub const fn new(realm_id: RealmId, code: ApiErrorCode, rule: &'static str) -> Self {
        Self {
            realm_id,
            code,
            rule,
            current_revision: None,
            retained: None,
            diagnostic: None,
            native_runtime_refusal: None,
            action: code.default_action(),
        }
    }

    /// Name the type, field or state machine that refused.
    #[must_use]
    pub fn about(mut self, subject: &'static str) -> Self {
        self.diagnostic_mut().subject = Some(subject);
        self
    }

    /// Point at the structural node that refused. Never a value.
    #[must_use]
    pub fn located_at(mut self, path: impl Into<String>) -> Self {
        self.diagnostic_mut().at = Some(path.into());
        self
    }

    /// The type, field or state machine that refused, when one is named.
    #[must_use]
    pub fn subject(&self) -> Option<&'static str> {
        self.diagnostic
            .as_ref()
            .and_then(|diagnostic| diagnostic.subject)
    }

    /// The structural path of the offending node, when there is one.
    #[must_use]
    pub fn at(&self) -> Option<&str> {
        self.diagnostic
            .as_ref()
            .and_then(|diagnostic| diagnostic.at.as_deref())
    }

    fn diagnostic_mut(&mut self) -> &mut ErrorDiagnostic {
        self.diagnostic.get_or_insert_with(|| {
            Box::new(ErrorDiagnostic {
                retry_after_seconds: None,
                subject: None,
                at: None,
            })
        })
    }

    /// Attach a safe observation retry delay without asserting a quota reset.
    #[must_use]
    pub fn with_retry_after(mut self, seconds: u64) -> Self {
        self.diagnostic_mut().retry_after_seconds = Some(seconds);
        self
    }

    /// State what the caller can do about it.
    #[must_use]
    pub const fn advising(mut self, action: &'static str) -> Self {
        self.action = action;
        self
    }

    /// Preserve a validated native refusal without admitting arbitrary runtime
    /// output into the API envelope.
    #[must_use]
    pub fn with_native_runtime_refusal(mut self, refusal: NativeRuntimeRefusal) -> Self {
        self.native_runtime_refusal = Some(Box::new(refusal));
        self
    }

    /// Attach the revision the aggregate actually stands at.
    #[must_use]
    pub const fn with_revision(mut self, revision: Option<AggregateRevision>) -> Self {
        self.current_revision = revision;
        self
    }

    /// Attach the retained control-plane window.
    #[must_use]
    pub const fn with_retained(mut self, oldest: EventCursor, newest: EventCursor) -> Self {
        self.retained = Some((oldest, newest));
        self
    }

    /// The body this refusal serializes to.
    #[must_use]
    pub fn body(&self) -> ApiErrorBody {
        ApiErrorBody {
            retry_after_seconds: self
                .diagnostic
                .as_ref()
                .and_then(|value| value.retry_after_seconds),
            realm_id: self.realm_id,
            code: self.code,
            rule: self.rule,
            current_revision: self.current_revision,
            oldest_retained_cursor: self.retained.map(|(oldest, _)| oldest),
            newest_cursor: self.retained.map(|(_, newest)| newest),
            subject: self.subject(),
            at: self.at().map(str::to_owned),
            native_runtime_refusal: self.native_runtime_refusal.as_deref().cloned(),
            action: self.action,
        }
    }

    /// Refuse a domain rejection that reached the transport.
    ///
    /// Every variant is mapped deliberately. The mapping reads the *kind* of
    /// rejection and its structural fields — `subject` is a `&'static str`
    /// written in this workspace and `path` is a document path, never a
    /// document value — so the envelope can say which type refused and where,
    /// while still never echoing what was rejected.
    ///
    /// The catch-all is not a shrug. [`DomainError`] is `#[non_exhaustive]`, so
    /// a variant added in a later generation would otherwise stop this crate
    /// compiling; what it must not do is *pretend to classify*. It says
    /// plainly that classification was unavailable and where to look instead.
    #[must_use]
    pub fn from_domain(realm_id: RealmId, error: &DomainError) -> Self {
        match error {
            DomainError::RealmMismatch { .. } => Self::new(
                realm_id,
                ApiErrorCode::RealmMismatch,
                "the value belongs to another realm",
            ),
            DomainError::RevisionConflict { subject, found, .. } => Self::new(
                realm_id,
                ApiErrorCode::RevisionConflict,
                "the aggregate moved since the caller read it",
            )
            .about(subject)
            .with_revision(AggregateRevision::parse(*found).ok()),
            DomainError::MissingAuthority { subject, .. } => Self::new(
                realm_id,
                ApiErrorCode::Forbidden,
                "the acting authority is not sufficient for this operation",
            )
            .about(subject),
            DomainError::Terminal { subject } => Self::new(
                realm_id,
                ApiErrorCode::RevisionConflict,
                "the aggregate is terminal and immutable",
            )
            .about(subject)
            .advising("the aggregate is closed; act on its successor instead of reopening it"),
            // A value failed its own type's invariant. The type is safe to name
            // and is usually the whole answer: "invalid ExternalName" tells a
            // caller which field to look at without quoting what they sent.
            DomainError::Invalid { subject, .. } => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "a value did not satisfy the invariant of its type",
            )
            .about(subject)
            .advising("correct the named field to satisfy its type and send the request again"),
            // The same, one level in. `path` is structural — `steps[2].role`,
            // never what was in it — and it is the difference between "your
            // document was refused" and something a caller can go and look at.
            DomainError::InvalidAt { subject, path, .. } => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "a value inside the document did not satisfy its invariant",
            )
            .about(subject)
            .located_at(path.clone())
            .advising("correct the node named by `at` and send the document again"),
            // Not a malformed request: a well-formed one against a state that
            // does not accept it. The states are `&'static str`, so naming them
            // costs nothing and saves a round trip.
            DomainError::IllegalTransition { subject, from, to } => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "the aggregate does not accept this transition from the state it is in",
            )
            .about(subject)
            .advising(illegal_transition_action(from, to)),
            // Nothing is wrong with the request; something it depends on has
            // not been recorded yet.
            DomainError::MissingEvidence { subject, .. } => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "the operation requires evidence that has not been recorded",
            )
            .about(subject)
            .advising("record the evidence this operation requires, then retry"),
            // The one refusal that must stay vague about *what* it saw, and can
            // still be exact about *where*.
            DomainError::SensitiveMaterial { path } => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "the document carries credential, token or unredacted personal material",
            )
            .about("SensitiveMaterial")
            .located_at(path.clone())
            .advising("remove or redact the node named by `at`; its value is never echoed back"),
            _ => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "a domain rule refused the request and this build cannot classify it",
            )
            .advising(
                "this daemon is older than the rule that refused; check the daemon log for the \
                 domain error, and upgrade the daemon so the refusal can be classified",
            ),
        }
    }

    /// Refuse a repository rejection.
    #[must_use]
    pub fn from_repository(realm_id: RealmId, error: &RepositoryError) -> Self {
        match error {
            RepositoryError::Domain(domain) => Self::from_domain(realm_id, domain),
            RepositoryError::NotFound { .. } => Self::new(
                realm_id,
                ApiErrorCode::NotFound,
                "the addressed aggregate does not exist in this realm",
            ),
            // A uniqueness, immutability or ordering rule refused the write. From
            // the transport's side that is always "you were working from a state
            // that has moved", which is what a revision conflict says.
            //
            // Which rule, on which aggregate, is now told to the caller as well
            // as logged. The previous reading withheld both to stop a client
            // enumerating the store's rules, and the cost of that was paid in
            // ASMA-8190: an operator retried one Core Team route apply twice
            // against `current_revision: null` and "a persistence rule refused
            // the write", which names nothing to re-read and nothing to change.
            // Both fields are `&'static str` authored in this workspace, so
            // there is no caller data in either; what they expose is a design
            // detail, and a refusal nobody can act on is the worse trade.
            RepositoryError::Conflict { subject, rule } => {
                warn!(
                    realm_id = %realm_id,
                    subject = %subject,
                    rule = %rule,
                    "a persistence rule refused a write"
                );
                Self::new(realm_id, ApiErrorCode::RevisionConflict, rule).about(subject)
            }
            // Which ceiling bound is a fact about this Realm's configuration and
            // its current load, so it is logged for the operator who runs the
            // plane and withheld from the caller who hit it. One static rule for
            // all four scopes: a client's response is the same either way, and
            // the difference is only actionable from inside.
            RepositoryError::CapacityExhausted { scope } => {
                warn!(scope, "an admission was refused for spent capacity");
                Self::new(
                    realm_id,
                    ApiErrorCode::CapacityExhausted,
                    "a configured concurrency ceiling is currently spent",
                )
            }
            // The same answer the native memory path gives, because it is the same
            // fact: this project's subject is not Kontor's to write yet. A retry
            // cannot change it, so it is never spelled as a conflict.
            RepositoryError::AuthorityWithheld { subject } => {
                warn!(
                    realm_id = %realm_id,
                    subject = %subject,
                    "a write was refused because a legacy system still owns the subject"
                );
                Self::new(
                    realm_id,
                    ApiErrorCode::Forbidden,
                    "the legacy system still owns this project's subject",
                )
            }
            RepositoryError::CrossProject { .. } => Self::new(
                realm_id,
                ApiErrorCode::NotFound,
                "the reference names a row in another project",
            ),
            // The detail carries SQLite's own text. It stays out of the envelope.
            RepositoryError::Backend { .. } => Self::new(
                realm_id,
                ApiErrorCode::Unavailable,
                "the control-plane store could not answer",
            ),
            // `RepositoryError` is `#[non_exhaustive]`, so this arm exists to
            // keep a newer store from breaking this crate. It must not pretend
            // to have classified anything: it says so, and it writes the detail
            // where an operator can actually find it.
            other => {
                warn!(
                    realm_id = %realm_id,
                    detail = %other,
                    "the store refused an operation with no mapped refusal"
                );
                Self::new(
                    realm_id,
                    ApiErrorCode::Unavailable,
                    "the control-plane store refused the operation and this build cannot classify it",
                )
                .advising(
                    "check the daemon log for the store error this refusal was logged with, and \
                     upgrade the daemon so the refusal can be classified",
                )
            }
        }
    }

    /// Refuse a runtime rejection.
    ///
    /// Every variant that names a capability or a binding maps to the code the
    /// contract owes it. A transport failure is reported as exactly that — a fact
    /// about the channel — and never as a statement about the work.
    #[must_use]
    pub fn from_runtime(realm_id: RealmId, error: &RuntimeError) -> Self {
        match error {
            RuntimeError::UnsupportedCapability { .. } => Self::new(
                realm_id,
                ApiErrorCode::UnsupportedCapability,
                "this session's runtime never declared that operation",
            ),
            RuntimeError::InsufficientTrust { .. } => Self::new(
                realm_id,
                ApiErrorCode::UnsupportedCapability,
                "this session's runtime may be observed but not driven",
            ),
            RuntimeError::StaleBinding { .. }
            | RuntimeError::CorrelationFailed
            | RuntimeError::SessionAlreadyBound { .. } => Self::new(
                realm_id,
                ApiErrorCode::StaleBinding,
                "the binding no longer names a session this runtime will act on",
            ),
            RuntimeError::TimelineRefetchRequired { .. } => Self::new(
                realm_id,
                ApiErrorCode::TimelineRefetchRequired,
                "the session's content must be read again from the runtime",
            ),
            RuntimeError::DuplicateMessage { .. } | RuntimeError::PermissionConflict { .. } => {
                Self::new(
                    realm_id,
                    ApiErrorCode::IdempotencyConflict,
                    "the identifier was already used to commit a different effect",
                )
            }
            RuntimeError::InvalidCursor { .. } => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "the runtime cursor was not issued for this session",
            ),
            RuntimeError::LimitExceeded {
                subject: "concurrent sessions",
                ..
            } => Self::new(
                realm_id,
                ApiErrorCode::CapacityExhausted,
                "this runtime's concurrent-session capacity is currently spent",
            ),
            RuntimeError::LimitExceeded { .. } => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "one bounded part of the request exceeds the runtime's declared limit",
            ),
            RuntimeError::ProviderUnavailable { .. } => Self::new(
                realm_id,
                ApiErrorCode::CapacityExhausted,
                "the selected provider is under a temporary operational outage",
            )
            .advising(
                "use an authorized eligible fallback route or retry after the provider recovers",
            ),
            RuntimeError::DeliveryConfirmationUnknown { .. } => Self::new(
                realm_id,
                ApiErrorCode::DeliveryUnconfirmed,
                "the message may have reached the session but canonical history has not confirmed its position",
            )
            .advising(
                "read the canonical session timeline for this exact idempotency key; do not resend until that read proves the outcome",
            ),
            RuntimeError::Transport { .. } => Self::new(
                realm_id,
                ApiErrorCode::Unavailable,
                "the session's runtime could not be reached",
            ),
            RuntimeError::CallerAgentNotFound { caller_agent_id } => Self::new(
                realm_id,
                ApiErrorCode::PlacementBlocked,
                "Paseo refused the launch because the configured caller agent does not exist",
            )
            .advising(
                "remove the stale native caller or select a caller owned by this epic, then resume the exact queued run",
            )
            .with_native_runtime_refusal(NativeRuntimeRefusal::CallerAgentNotFound {
                caller_agent_id: caller_agent_id.clone(),
            }),
            // A workspace refusal is a *placement* fact, not a channel fact: the
            // runtime answered, and what it said is that the root it was handed
            // is not one it will work in. Letting it fall to the catch-all below
            // reported it as "refused the operation" with the rule discarded and
            // nothing logged, which is what made this class of defect cost a
            // source read and an experiment to diagnose.
            // A seat whose placement this process cannot name is not a runtime
            // that refused: it is a *seat that cannot be driven*, and the two
            // want opposite things from an operator. This one is recoverable —
            // a reconciliation re-proves the placement from the live runtime —
            // and saying so is the difference between "retry" and "investigate".
            RuntimeError::WorkspaceBindingRequired => {
                warn!(
                    realm_id = %realm_id,
                    "a seat operation was attempted with no proved workspace placement"
                );
                Self::new(
                    realm_id,
                    ApiErrorCode::PlacementBlocked,
                    "this process cannot prove where the session's seat is placed",
                )
                .advising(
                    "re-prove the seat's workspace placement, then resume the exact queued run",
                )
            }
            // The rule is told, not only logged, for the same reason a
            // persistence conflict now names its subject: a dozen distinct
            // workspace conditions collapse to this one variant, and
            // "the runtime will not work in the workspace this realm asked
            // for" says nothing about which. ASMA-8190 is what that costs — an
            // apply refused a route its own preview had accepted, and the
            // operator had no way to tell a busy terminal from a moved
            // directory from an uncorrelated setup census. Every rule here is
            // an `&'static str` written in this workspace.
            RuntimeError::WorkspaceMismatch { rule } => {
                warn!(
                    realm_id = %realm_id,
                    rule = %rule,
                    "runtime refused the workspace this realm asked for"
                );
                Self::new(realm_id, ApiErrorCode::UnsupportedCapability, rule)
            }
            RuntimeError::WorkspacePreparationFailed { rule } => {
                warn!(
                    realm_id = %realm_id,
                    rule = %rule,
                    "runtime could not prepare the declared task checkout"
                );
                Self::new(
                    realm_id,
                    ApiErrorCode::PlacementBlocked,
                    "the declared task checkout could not be prepared safely",
                )
                .advising(
                    "correct the declared worktree or its Git branch conflict, then retry the same materialization",
                )
            }
            // A predecessor the runtime will not agree is finished is a
            // *conflict*, not a server defect. It fell into the catch-all below
            // and answered an unclassified 503, which told an operator to check
            // the daemon log and upgrade the build -- for a refusal that is
            // working exactly as designed. The 2026-08-22 incident's permanent
            // correction is that this becomes a typed 409.
            RuntimeError::ReplacementNotEvidenced { rule } => Self::new(
                realm_id,
                ApiErrorCode::RevisionConflict,
                "the runtime does not agree the cited predecessor is finished",
            )
            .advising(rule),
            // The same lesson as the arm above, found the same way. A claimant the
            // runtime already admitted elsewhere is a *conflict* the caller can
            // act on: release or reuse the seat it already holds. Answering an
            // unclassified 503 told an operator to upgrade the daemon for a
            // refusal that is working exactly as designed, and left an
            // epic-level claim preview looking like a server defect.
            RuntimeError::SlotAlreadyAdmitted { rule } => Self::new(
                realm_id,
                ApiErrorCode::RevisionConflict,
                "the runtime has already admitted this role slot",
            )
            .advising(rule),
            // A launch the runtime will not admit because it cannot prove a
            // required capability is not unavailability either: retrying will
            // not help until the capability is provable.
            RuntimeError::LaunchNotAdmitted { rule } => Self::new(
                realm_id,
                ApiErrorCode::UnsupportedCapability,
                "the runtime would not admit this launch",
            )
            .advising(rule),
            RuntimeError::Domain(domain) => Self::from_domain(realm_id, domain),
            // Whatever is left is genuinely unclassified, and it says so in the
            // log rather than only in the answer: an operator who sees this
            // refusal and finds nothing written down has to read the adapter
            // source to learn what happened.
            other => {
                warn!(
                    realm_id = %realm_id,
                    detail = %other,
                    "runtime refused an operation with no mapped refusal"
                );
                Self::new(
                    realm_id,
                    ApiErrorCode::Unavailable,
                    "the runtime refused the operation and this build cannot classify it",
                )
                .advising(
                    "check the daemon log for the runtime error this refusal was logged with, and \
                     upgrade the daemon so the refusal can be classified",
                )
            }
        }
    }

    /// Refuse a control-plane cursor that is outside the retained history.
    #[must_use]
    pub fn resnapshot(realm_id: RealmId, window: (RealmCursor, RealmCursor)) -> Self {
        Self::new(
            realm_id,
            ApiErrorCode::ResnapshotRequired,
            "the requested position is outside the retained control-plane history",
        )
        .with_retained(window.0.cursor, window.1.cursor)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = self.body();
        let retry_after = body.retry_after_seconds;
        let mut response = (self.code.status(), Json(body)).into_response();
        if let Some(seconds) = retry_after
            && let Ok(value) = axum::http::HeaderValue::from_str(&seconds.to_string())
        {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use kontor_core::id::AggregateRevision;

    use super::*;

    /// A refusal an operator cannot act on is a defect, not discretion.
    ///
    /// ASMA-8190: one Core Team route apply was retried twice against
    /// `current_revision: null` and "a persistence rule refused the write",
    /// which names nothing to re-read. Both fields are `&'static str` written
    /// in this workspace, so neither carries caller data.
    #[test]
    fn a_persistence_conflict_names_its_subject_and_rule() {
        let realm = RealmId::generate();
        let refusal = ApiError::from_repository(
            realm,
            &kontor_core::repository::RepositoryError::Conflict {
                subject: "native container binding",
                rule: "this topology node is bound to another native container",
            },
        );
        assert_eq!(refusal.code, ApiErrorCode::RevisionConflict);
        assert_eq!(
            refusal.rule,
            "this topology node is bound to another native container"
        );
        assert_eq!(refusal.subject(), Some("native container binding"));
    }

    /// A dozen workspace conditions share one variant; the rule is what tells
    /// a busy terminal from a moved directory from an uncorrelated census.
    #[test]
    fn a_workspace_refusal_names_the_rule_that_fired() {
        let realm = RealmId::generate();
        let refusal = ApiError::from_runtime(
            realm,
            &RuntimeError::WorkspaceMismatch {
                rule: "the workspace still reports terminals or another directory",
            },
        );
        assert_eq!(refusal.code, ApiErrorCode::UnsupportedCapability);
        assert_eq!(
            refusal.rule,
            "the workspace still reports terminals or another directory"
        );
    }

    /// The 2026-08-22 lesson, applied to the two refusals that were still
    /// falling through: a conflict and a capability refusal are actionable
    /// answers, and telling an operator to upgrade the daemon for either one
    /// hides a working fence behind a server defect.
    #[test]
    fn an_already_admitted_slot_is_a_conflict_not_an_unclassified_outage() {
        let realm = RealmId::generate();
        let refusal = ApiError::from_runtime(
            realm,
            &RuntimeError::SlotAlreadyAdmitted {
                rule: "the claimant already belongs to another Kontor or native seat",
            },
        );
        assert_eq!(refusal.code, ApiErrorCode::RevisionConflict);
        assert_ne!(
            refusal.code,
            ApiErrorCode::Unavailable,
            "an already-admitted claimant is not an outage"
        );
        assert!(
            refusal.action.contains("already belongs"),
            "the runtime's own rule is carried as the action: {refusal:?}"
        );
    }

    #[test]
    fn an_unadmitted_launch_is_an_unsupported_capability_not_an_outage() {
        let realm = RealmId::generate();
        let refusal = ApiError::from_runtime(
            realm,
            &RuntimeError::LaunchNotAdmitted {
                rule: "this Paseo does not advertise providerOptionsApplied",
            },
        );
        assert_eq!(refusal.code, ApiErrorCode::UnsupportedCapability);
        assert_ne!(refusal.code, ApiErrorCode::Unavailable);
    }

    #[test]
    fn a_realm_mismatch_is_reported_as_such_and_echoes_no_payload() {
        let here = RealmId::generate();
        let elsewhere = RealmId::generate();
        let refusal = ApiError::from_domain(
            here,
            &DomainError::RealmMismatch {
                expected: here,
                found: elsewhere,
            },
        );
        assert_eq!(refusal.code, ApiErrorCode::RealmMismatch);
        assert_eq!(refusal.code.status(), StatusCode::CONFLICT);
        let body = serde_json::to_string(&refusal.body()).expect("the envelope serializes");
        assert!(
            body.contains(&here.to_string()),
            "a refusal names the realm that refused"
        );
        assert!(
            !body.contains(&elsewhere.to_string()),
            "and says nothing about the realm the value claimed to come from"
        );
    }

    #[test]
    fn a_spent_ceiling_is_its_own_refusal_and_names_no_scope() {
        let realm = RealmId::generate();
        let refusal = ApiError::from_repository(
            realm,
            &RepositoryError::CapacityExhausted { scope: "account" },
        );

        // Its own code, and a status a client already knows to back off on.
        assert_eq!(refusal.code, ApiErrorCode::CapacityExhausted);
        assert_eq!(refusal.code.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            refusal.rule,
            "a configured concurrency ceiling is currently spent"
        );

        // The scope reached the log, never the envelope. Nothing about which
        // ceiling bound, and no revision to mislead a caller into retrying.
        let body = serde_json::to_string(&refusal.body()).expect("the envelope serializes");
        for scope in ["account", "project", "global", "goal"] {
            assert!(
                !body.contains(scope),
                "a spent ceiling must not disclose which one: {body}"
            );
        }
        assert_eq!(refusal.current_revision, None);

        // And an actual conflict is still a conflict: the two are not merged in
        // either direction.
        let conflict = ApiError::from_repository(
            realm,
            &RepositoryError::Conflict {
                subject: "task",
                rule: "a uniqueness rule refused",
            },
        );
        assert_eq!(conflict.code, ApiErrorCode::RevisionConflict);
        assert_eq!(conflict.code.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn a_runtime_session_ceiling_is_capacity_not_a_malformed_request() {
        let refusal = ApiError::from_runtime(
            RealmId::generate(),
            &RuntimeError::LimitExceeded {
                subject: "concurrent sessions",
                limit: 64,
            },
        );

        assert_eq!(refusal.code, ApiErrorCode::CapacityExhausted);
        assert_eq!(refusal.code.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            refusal.rule,
            "this runtime's concurrent-session capacity is currently spent"
        );
    }

    #[test]
    fn a_missing_native_caller_is_an_actionable_exact_placement_refusal() {
        let caller =
            ExternalId::parse("619d6f8a-0bbc-4b8d-a3ad-8e38a0cd8234").expect("a native caller id");
        let refusal = ApiError::from_runtime(
            RealmId::generate(),
            &RuntimeError::CallerAgentNotFound {
                caller_agent_id: caller.clone(),
            },
        );

        assert_eq!(refusal.code, ApiErrorCode::PlacementBlocked);
        assert!(refusal.action.contains("resume the exact queued run"));
        let body = serde_json::to_value(refusal.body()).expect("the refusal serializes");
        assert_eq!(
            body["native_runtime_refusal"]["kind"],
            "caller_agent_not_found"
        );
        assert_eq!(
            body["native_runtime_refusal"]["caller_agent_id"],
            caller.as_str()
        );
    }

    #[test]
    fn an_unplaced_seat_refuses_with_placement_not_a_settle_instruction() {
        let refusal =
            ApiError::from_runtime(RealmId::generate(), &RuntimeError::WorkspaceBindingRequired);

        assert_eq!(refusal.code, ApiErrorCode::PlacementBlocked);
        assert!(refusal.action.contains("resume the exact queued run"));
        assert!(!refusal.action.contains("settle"));
    }

    #[test]
    fn a_revision_conflict_carries_the_revision_the_caller_needs() {
        let realm = RealmId::generate();
        let refusal = ApiError::from_domain(
            realm,
            &DomainError::RevisionConflict {
                subject: "task",
                expected: 3,
                found: 7,
            },
        );
        assert_eq!(refusal.code, ApiErrorCode::RevisionConflict);
        assert_eq!(
            refusal.current_revision,
            Some(AggregateRevision::parse(7).expect("a positive revision")),
            "the caller is answered with the revision it must present next"
        );
    }

    #[test]
    fn a_backend_failure_never_carries_the_backends_own_text() {
        let realm = RealmId::generate();
        let refusal = ApiError::from_repository(
            realm,
            &RepositoryError::Backend {
                detail: "no such column: secret_token".to_owned(),
            },
        );
        assert_eq!(refusal.code, ApiErrorCode::Unavailable);
        let body = serde_json::to_string(&refusal.body()).expect("the envelope serializes");
        assert!(
            !body.contains("secret_token"),
            "a backend detail may name a column, and a column may name anything"
        );
    }

    #[test]
    fn checkout_preparation_is_a_typed_placement_block_not_a_runtime_outage() {
        let realm = RealmId::generate();
        let refusal = ApiError::from_runtime(
            realm,
            &RuntimeError::WorkspacePreparationFailed {
                rule: "the declared worktree is checked out on a different branch",
            },
        );

        assert_eq!(refusal.code, ApiErrorCode::PlacementBlocked);
        assert_eq!(refusal.code.status(), StatusCode::CONFLICT);
        assert_eq!(
            refusal.action,
            "correct the declared worktree or its Git branch conflict, then retry the same materialization"
        );
    }

    #[test]
    fn an_unknown_message_confirmation_never_claims_nothing_changed() {
        let refusal = ApiError::from_runtime(
            RealmId::generate(),
            &RuntimeError::DeliveryConfirmationUnknown {
                rule: "canonical history did not finish",
            },
        );

        // OG-058. This used to be reported as `unavailable`, and the warning
        // lived only in the action text. That is not enough: this enum's own
        // contract is that a client branches on the code "and on nothing else",
        // so a caller doing exactly what it was told saw the code whose
        // meaning is *nothing happened* and resent.
        assert_eq!(refusal.code, ApiErrorCode::DeliveryUnconfirmed);
        assert_ne!(
            refusal.code,
            ApiErrorCode::Unavailable,
            "a delivery that may have landed must not share a code with one that never left"
        );
        assert_eq!(refusal.code.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            refusal.rule,
            "the message may have reached the session but canonical history has not confirmed its position"
        );
        assert!(refusal.action.contains("do not resend"));
        assert!(!refusal.action.contains("nothing was changed"));
    }

    #[test]
    fn usage_throttle_returns_the_actual_retry_instruction_in_body_and_header() {
        let error = ApiError::new(
            RealmId::generate(),
            ApiErrorCode::ProviderUsageThrottled,
            "the usage endpoint throttled observation requests",
        )
        .with_retry_after(2312);
        assert_eq!(error.body().retry_after_seconds, Some(2312));
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["retry-after"], "2312");
    }

    /// The two codes are told apart by the one sentence a caller acts on.
    #[test]
    fn only_the_pre_effect_code_may_promise_that_nothing_changed() {
        assert!(
            ApiErrorCode::Unavailable
                .default_action()
                .contains("nothing was changed"),
            "the channel-only code keeps its promise; that is what makes it useful"
        );
        assert!(
            !ApiErrorCode::DeliveryUnconfirmed
                .default_action()
                .contains("nothing was changed"),
            "the post-effect code must never make that promise"
        );
        assert!(
            ApiErrorCode::DeliveryUnconfirmed
                .default_action()
                .contains("never resend under a new one"),
            "and it has to name the unsafe act, not merely decline to promise"
        );
    }
}
