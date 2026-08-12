//! `kontor-calendar` — Work windows, holiday imports, drain state and scoped overrides
//!
//! This crate is the **only** place a calendar is resolved. The scheduler
//! consumes [`kontor_scheduler::model::CalendarAdmission`] and never parses a
//! window, a zone, a holiday feed or an expiry; the store records what was
//! applied; the clients render the answer. One resolver means one answer, and an
//! answer that two components could each compute is an answer they can each
//! compute differently.
//!
//! ## The rule that shapes everything else
//!
//! **Absence of a calendar is not a closed calendar.** A project with no active
//! assignment resolves to `unrestricted` — no timezone, no profile, no holiday
//! source, no time limit — and an override on such a project is redundant rather
//! than authoritative. Only a *configured* calendar can close anything, and only
//! a refusal by a configured calendar can be overridden.
//!
//! ## Resolution
//!
//! [`resolve`] is a pure function of [`ResolutionRequest`]. It takes the
//! coordinator's instant as an input and has no clock of its own, no network and
//! no store: everything it reads has already been applied locally. The order of
//! its steps is a rule in itself and is documented on the function.
//!
//! ## Imports
//!
//! Retrieval, parsing and applying are three separate steps on purpose:
//!
//! 1. [`fetch::retrieve`] gets bytes, bounded in size and time. It is the only
//!    part of this crate that touches a network, and nothing on the dispatch
//!    path calls it.
//! 2. [`import::preview`] is pure. It parses one of three documented shapes —
//!    Nager holiday JSON, GOV.UK bank-holiday JSON, or an iCalendar document of
//!    bounded all-day events — normalizes it, and records what it refused. It
//!    writes nothing, so a preview a reviewer rejects has changed no calendar.
//! 3. [`import::plan`] turns a preview and the currently applied state into the
//!    immutable revisions [`kontor_core::repository::CalendarRepository::apply_holiday_import`]
//!    writes in one transaction.
//!
//! ## What this crate deliberately does not do
//!
//! It does not schedule. It answers "what does the calendar say", and the
//! scheduler decides what to do about that with every other guardrail beside it.
//! It does not expand RFC 5545 recurrence: a recurring event is refused with a
//! stable warning code rather than half-expanded. And it never consults a
//! client's clock — the instant is always the coordinator's.

pub mod fetch;
pub mod import;
pub mod resolve;

pub use import::{
    ImportApplication, ImportDiff, ImportPreview, ImportRequest, ImportTarget, NormalizedHoliday,
    diff, plan, preview,
};
pub use resolve::{ResolutionRequest, resolve, resolve_scoped};

/// Every refusal this crate can produce.
///
/// Like [`kontor_core::DomainError`], the payload is structural: a subject and a
/// rule, both static. A rejected holiday feed, an ICS document or a URL is never
/// echoed into an error, a log or a test assertion.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CalendarError {
    /// The domain refused a value this crate built from a source document.
    #[error(transparent)]
    Domain(#[from] kontor_core::DomainError),

    /// A child scope declared a window its parent calendar does not cover.
    ///
    /// Inheritance narrows. Widening is a policy change, and a policy change
    /// needs the same approved, bounded, revocable override that any other
    /// out-of-hours work needs.
    #[error("a child scope widens its inherited calendar without an approved override")]
    WidenedWithoutApproval,

    /// A retrieved document was not the shape its importer expects.
    #[error("malformed {subject}: {rule}")]
    Malformed {
        /// Which document.
        subject: &'static str,
        /// What was wrong with it, as a rule and never as a value.
        rule: &'static str,
    },

    /// A document, or the number of entries in it, exceeded the import bounds.
    #[error("the retrieved document exceeds the bounded import size")]
    TooLarge,

    /// The document produced more refusals than a usable document does.
    #[error("the document produced more warnings than one import may record")]
    TooManyWarnings,

    /// Retrieval failed. Carries no URL, no host and no response body.
    #[error("retrieval failed: {rule}")]
    Retrieval {
        /// Why, as a stable rule.
        rule: &'static str,
    },
}

/// Convenience alias for this crate's fallible operations.
pub type CalendarResult<T> = Result<T, CalendarError>;
