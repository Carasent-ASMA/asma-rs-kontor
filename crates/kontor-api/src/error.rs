//! One error envelope, one closed vocabulary of codes, and nothing else on the
//! wire.
//!
//! Every refusal this API can produce is an [`ApiError`]. It carries the Realm it
//! was refused in, a stable machine code and a *static* rule — never the request
//! body, never a token, never a runtime URL, never a line of session content. A
//! caller can therefore log the whole envelope, and a test can assert on it,
//! without either of them becoming a place secrets accumulate.
//!
//! The one structured detail any code carries is a position or a revision: a
//! number the caller already had or is owed.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kontor_core::id::{AggregateRevision, EventCursor, RealmId};
use kontor_core::realm::RealmCursor;
use kontor_core::repository::RepositoryError;
use kontor_core::{DomainError, closed_enum};
use kontor_runtime::adapter::RuntimeError;
use serde::Serialize;
use tracing::warn;
use utoipa::ToSchema;

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
        /// The binding's frozen capability set does not cover this operation.
        UnsupportedCapability => "unsupported_capability",
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
        /// A dependency could not be reached. A fact about the channel only.
        Unavailable => "unavailable",
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
            | Self::StaleBinding
            | Self::TimelineRefetchRequired => StatusCode::CONFLICT,
            // The request is well formed and understood; this runtime simply
            // cannot do it. That is not a server defect, so it is not a 5xx.
            Self::UnsupportedCapability | Self::HandoffUnsettled => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            // The position the caller wants is genuinely gone.
            Self::ResnapshotRequired => StatusCode::GONE,
            // The request is well formed and the state it presented is current;
            // what is missing is an accounting the caller must supply or excuse.
            // Same reasoning as `UnsupportedCapability`: not a server defect.
            Self::RoleSlotUnbound => StatusCode::UNPROCESSABLE_ENTITY,
            // Nothing is wrong with the request or the state it presented, so a
            // 4xx that blames either would misdirect. "Too many requests" is
            // what a spent ceiling is, and it is the status a client already
            // knows to back off and retry on.
            Self::CapacityExhausted => StatusCode::TOO_MANY_REQUESTS,
            Self::ReconciliationPending | Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

/// The JSON body every refusal is reported with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ApiErrorBody {
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
        }
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
            realm_id: self.realm_id,
            code: self.code,
            rule: self.rule,
            current_revision: self.current_revision,
            oldest_retained_cursor: self.retained.map(|(oldest, _)| oldest),
            newest_cursor: self.retained.map(|(_, newest)| newest),
        }
    }

    /// Refuse a domain rejection that reached the transport.
    ///
    /// The mapping reads the *kind* of rejection, never its message, so a new
    /// domain variant degrades to an honest `invalid_request` instead of being
    /// reported as something more specific than it is.
    #[must_use]
    pub fn from_domain(realm_id: RealmId, error: &DomainError) -> Self {
        match error {
            DomainError::RealmMismatch { .. } => Self::new(
                realm_id,
                ApiErrorCode::RealmMismatch,
                "the value belongs to another realm",
            ),
            DomainError::RevisionConflict { found, .. } => Self::new(
                realm_id,
                ApiErrorCode::RevisionConflict,
                "the aggregate moved since the caller read it",
            )
            .with_revision(AggregateRevision::parse(*found).ok()),
            DomainError::MissingAuthority { .. } => Self::new(
                realm_id,
                ApiErrorCode::Forbidden,
                "the acting authority is not sufficient for this operation",
            ),
            DomainError::Terminal { .. } => Self::new(
                realm_id,
                ApiErrorCode::RevisionConflict,
                "the aggregate is terminal and immutable",
            ),
            _ => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "the request was refused by a domain rule",
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
            RepositoryError::Conflict { .. } => Self::new(
                realm_id,
                ApiErrorCode::RevisionConflict,
                "a persistence rule refused the write against the presented state",
            ),
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
            _ => Self::new(
                realm_id,
                ApiErrorCode::Unavailable,
                "the control-plane store refused the operation",
            ),
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
            RuntimeError::LimitExceeded { .. } => Self::new(
                realm_id,
                ApiErrorCode::InvalidRequest,
                "the request is larger than this session's runtime declared",
            ),
            RuntimeError::Transport { .. } => Self::new(
                realm_id,
                ApiErrorCode::Unavailable,
                "the session's runtime could not be reached",
            ),
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
                    ApiErrorCode::StaleBinding,
                    "this process cannot prove where the session's seat is placed",
                )
            }
            RuntimeError::WorkspaceMismatch { rule } => {
                warn!(
                    realm_id = %realm_id,
                    rule = %rule,
                    "runtime refused the workspace this realm asked for"
                );
                Self::new(
                    realm_id,
                    ApiErrorCode::UnsupportedCapability,
                    "the runtime will not work in the workspace this realm asked for",
                )
            }
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
                    "the session's runtime refused the operation",
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
        (self.code.status(), Json(self.body())).into_response()
    }
}

#[cfg(test)]
mod tests {
    use kontor_core::id::AggregateRevision;

    use super::*;

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
}
