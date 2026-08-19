//! Which system may write one project's memory or backlog.
//!
//! Authority is a fact about a `(project, subject)` pair, never about the Realm.
//! A Realm holds projects whose facts arrived by different routes at different
//! times: one created directly in Kontor, one still being imported out of
//! AgentsRoom. A single switch cannot describe both, and a global one would
//! silently claim authority over every project that had not asked for it.
//!
//! Two things are therefore kept apart:
//!
//! * [`SubjectOrigin`] — how this subject's facts *entered*. Recorded once, at
//!   project creation, and never rewritten. It decides whether cutover is a
//!   meaningful operation at all.
//! * [`SubjectAuthority`] — who may write it *now*. For a native subject this is
//!   Kontor from the first instant. For an imported one it moves exactly once,
//!   and only on the complete evidence set.
//!
//! A native subject is never asked to produce a freeze, an export or an empty
//! import manifest to earn what it already had.

use crate::id::{AggregateRevision, ContentHash, ProjectId, Timestamp};

closed_enum! {
    /// The closed set of facts authority is tracked for.
    ///
    /// Memory and backlog are separate subjects because they migrate
    /// independently: a project's backlog can already be Kontor's while its
    /// memory is still being imported.
    AuthoritySubject, "AuthoritySubject" {
        /// The project's memory ledger.
        Memory => "memory",
        /// The project's mini-project/task graph and its lifecycle.
        Backlog => "backlog",
    }
}

closed_enum! {
    /// How a project/subject's facts entered Kontor. Immutable.
    SubjectOrigin, "SubjectOrigin" {
        /// Created in Kontor. Writable immediately; cutover never applies.
        KontorNative => "kontor_native",
        /// Carried over from a legacy system, and not yet imported and switched.
        ///
        /// The spelling names Kontor's own state rather than the system the facts
        /// came from. *Which* legacy system it was is recorded on the import
        /// manifest's `source`, where it belongs: this value reaches the
        /// model-facing tool vocabulary through `projects:ensure`, and that
        /// vocabulary is not allowed to name a tracker or a legacy backlog.
        LegacyPending => "legacy_pending",
    }
}

closed_enum! {
    /// Who may write this project/subject right now.
    SubjectAuthority, "SubjectAuthority" {
        /// The legacy system still owns it; Kontor reads and previews only.
        Agentsroom => "agentsroom",
        /// Kontor owns it.
        Kontor => "kontor",
    }
}

impl SubjectOrigin {
    /// The authority a row of this origin is created at.
    #[must_use]
    pub const fn initial_authority(self) -> SubjectAuthority {
        match self {
            Self::KontorNative => SubjectAuthority::Kontor,
            Self::LegacyPending => SubjectAuthority::Agentsroom,
        }
    }

    /// Whether freeze, import and switch mean anything for this origin.
    ///
    /// A native subject answers `false`, which is why those operations refuse it
    /// as inapplicable instead of synthesizing an empty ceremony.
    #[must_use]
    pub const fn permits_cutover(self) -> bool {
        matches!(self, Self::LegacyPending)
    }
}

/// One row of the project/subject authority ledger.
///
/// The four evidence fields are all absent or all present: they are written
/// together by the one guarded switch, and a native row never carries any of
/// them. `source_frozen_at` is the exception that is set on its own, by the
/// operator attestation the switch later requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSubjectAuthority {
    /// The project.
    pub project_id: ProjectId,
    /// The subject.
    pub subject: AuthoritySubject,
    /// How its facts entered. Immutable.
    pub origin: SubjectOrigin,
    /// Who may write it now.
    pub authority: SubjectAuthority,
    /// Bumped by the attestation and by the switch.
    pub revision: AggregateRevision,
    /// When the operator attested the legacy source frozen.
    pub source_frozen_at: Option<Timestamp>,
    /// The import hash the switch was granted against.
    pub final_import_hash: Option<ContentHash>,
    /// The hash recomputed from stored Kontor state at switch time.
    pub readback_hash: Option<ContentHash>,
    /// When authority moved.
    pub switched_at: Option<Timestamp>,
}

impl ProjectSubjectAuthority {
    /// Whether Kontor may write this subject.
    #[must_use]
    pub const fn writable_by_kontor(&self) -> bool {
        matches!(self.authority, SubjectAuthority::Kontor)
    }
}

#[cfg(test)]
mod tests {
    use super::{SubjectAuthority, SubjectOrigin};

    #[test]
    fn native_origins_start_writable_and_refuse_cutover() {
        assert_eq!(
            SubjectOrigin::KontorNative.initial_authority(),
            SubjectAuthority::Kontor
        );
        assert!(!SubjectOrigin::KontorNative.permits_cutover());
    }

    #[test]
    fn pending_origins_start_legacy_and_permit_cutover() {
        assert_eq!(
            SubjectOrigin::LegacyPending.initial_authority(),
            SubjectAuthority::Agentsroom
        );
        assert!(SubjectOrigin::LegacyPending.permits_cutover());
    }

    #[test]
    fn spellings_round_trip() {
        for origin in SubjectOrigin::ALL {
            assert_eq!(&SubjectOrigin::parse(origin.as_str()).unwrap(), origin);
        }
        assert!(SubjectOrigin::parse("kontor").is_err());
    }
}
