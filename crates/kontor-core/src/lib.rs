//! `kontor-core` — Domain identifiers, states and commands for the Kontor control plane
//!
//! This crate owns the domain contract: identity, orthogonal lifecycle state,
//! versioned specifications, external-ticket policy, calendar state and command
//! receipts. It has no persistence, transport or runtime dependency; the
//! repository *ports* are declared here and implemented by `kontor-store`.
//!
//! Two rules shape every type in this crate:
//!
//! 1. **The core is generic.** Profile ids, phase ids, connector names, external
//!    status names and issue types are open data. No function in this crate may
//!    branch on a particular deployment's value — a seed profile id, one
//!    project's status name, one connector's transition id. Such values live
//!    only in test fixtures.
//! 2. **Invalid states are rejected before persistence.** Values are parsed into
//!    types that carry their invariant (`ContentHash`, `WorkProfileKey`,
//!    `CanonicalDocument`, …) rather than validated ad hoc at each call site.
//!
//! Errors never echo the value that failed validation: a rejected source
//! envelope, persona document or credential must not reach a log or a test
//! assertion. [`DomainError`] therefore carries only a static subject, a static
//! rule and — where useful — a structural path.

/// Declare a closed enum with one stable text spelling per variant.
///
/// The spelling is the persistence and wire form; `parse` is the only way back,
/// so an unknown value from SQL or JSON is rejected instead of defaulted.
///
/// Exported so the crates layered on top of this one declare their closed
/// domains the same way rather than hand-rolling a second, subtly different
/// parse/serialize pair. Every expansion refers to `$crate::DomainError`, so a
/// downstream enum rejects an unknown spelling with exactly this crate's error.
#[macro_export]
macro_rules! closed_enum {
    (
        $(#[$meta:meta])*
        $name:ident, $subject:literal {
            $( $(#[$variant_meta:meta])* $variant:ident => $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $( $(#[$variant_meta])* $variant, )+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )+ ];

            /// The stable spelling used in JSON and SQLite.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $( Self::$variant => $text, )+ }
            }

            /// Parse the stable spelling.
            ///
            /// # Errors
            /// Returns [`DomainError::Invalid`] for any other text.
            pub fn parse(text: &str) -> $crate::DomainResult<Self> {
                match text {
                    $( $text => Ok(Self::$variant), )+
                    _ => Err($crate::DomainError::invalid($subject, "is not a known value")),
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                use ::serde::de::Error as _;
                let text = <String as ::serde::Deserialize>::deserialize(deserializer)?;
                Self::parse(&text).map_err(D::Error::custom)
            }
        }
    };
}

pub mod calendar;
pub mod compaction;
pub mod consultation;
pub mod id;
pub mod realm;
pub mod receipt;
pub mod repository;
pub mod spec;
pub mod state;
pub mod ticket;

/// Every domain rejection in this crate.
///
/// The payload is deliberately structural: `subject` and `rule` are static
/// strings and `path` is a document path, never a document value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DomainError {
    /// A value did not satisfy the documented invariant of its type.
    #[error("invalid {subject}: {rule}")]
    Invalid {
        /// The type or field that rejected the value.
        subject: &'static str,
        /// The invariant that was violated.
        rule: &'static str,
    },
    /// A value inside a structured document violated an invariant.
    #[error("invalid {subject} at `{path}`: {rule}")]
    InvalidAt {
        /// The type or field that rejected the value.
        subject: &'static str,
        /// Structural path of the offending node (never its value).
        path: String,
        /// The invariant that was violated.
        rule: &'static str,
    },
    /// A state machine refused a transition.
    #[error("illegal {subject} transition: {from} -> {to}")]
    IllegalTransition {
        /// The state machine that refused.
        subject: &'static str,
        /// Current state.
        from: &'static str,
        /// Requested state.
        to: &'static str,
    },
    /// A terminal, immutable aggregate was asked to change.
    #[error("{subject} is terminal and immutable")]
    Terminal {
        /// The aggregate that is already closed.
        subject: &'static str,
    },
    /// A compare-and-swap saw a different revision than the caller expected.
    #[error("revision conflict on {subject}: expected {expected}, found {found}")]
    RevisionConflict {
        /// The aggregate whose revision moved.
        subject: &'static str,
        /// Revision the caller expected.
        expected: u64,
        /// Revision actually stored.
        found: u64,
    },
    /// The acting role or capability is not authorized for this operation.
    #[error("missing authority for {subject}: {rule}")]
    MissingAuthority {
        /// The operation that requires authority.
        subject: &'static str,
        /// Why the presented authority is insufficient.
        rule: &'static str,
    },
    /// Required evidence was absent.
    #[error("missing evidence for {subject}: {rule}")]
    MissingEvidence {
        /// The operation that requires evidence.
        subject: &'static str,
        /// Which evidence is missing.
        rule: &'static str,
    },
    /// An envelope, cursor or id belonged to a different Realm.
    ///
    /// Carries only the two Realm ids, never the envelope payload.
    #[error("realm mismatch: expected {expected}, found {found}")]
    RealmMismatch {
        /// The Realm this store is bound to.
        expected: crate::id::RealmId,
        /// The Realm the value claimed.
        found: crate::id::RealmId,
    },
    /// A document carried credentials, secrets or unredacted personal data.
    ///
    /// Only the structural path is reported; the value is never echoed.
    #[error("sensitive material rejected at `{path}`")]
    SensitiveMaterial {
        /// Structural path of the offending node.
        path: String,
    },
}

impl DomainError {
    /// Build an [`DomainError::Invalid`].
    #[must_use]
    pub const fn invalid(subject: &'static str, rule: &'static str) -> Self {
        Self::Invalid { subject, rule }
    }

    /// Build an [`DomainError::InvalidAt`].
    #[must_use]
    pub fn invalid_at(subject: &'static str, path: impl Into<String>, rule: &'static str) -> Self {
        Self::InvalidAt {
            subject,
            path: path.into(),
            rule,
        }
    }
}

/// Convenience alias for fallible domain operations.
pub type DomainResult<T> = Result<T, DomainError>;
