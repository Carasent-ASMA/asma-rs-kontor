//! How a caller names one epic or task on a public route.
//!
//! The confirmed Jira key is the primary public identifier, and the internal
//! UUID stays valid for every existing caller. A selector carries exactly those
//! two spellings and nothing else: it is not a place to smuggle a title, a
//! backlog code or a legacy item code, none of which identify a subject.
//!
//! The two grammars cannot collide. [`TrackerKey`] requires a leading uppercase
//! ASCII letter and never repairs case; a canonical v7 UUID begins with a
//! lowercase hexadecimal digit. So the parse order below is a matter of
//! determinism in the refusal message, never of correctness.
//!
//! Resolution of a key to a subject is deliberately *not* here. It needs the
//! project scope and the confirmed-binding ledger, so it belongs to the store's
//! single resolver; a selector only says which spelling the caller used.

use crate::branch::TrackerKey;
use crate::id::{MiniProjectId, TaskId};
use crate::{DomainError, DomainResult};

/// Build one selector type over an id type, so the two cannot drift apart.
macro_rules! selector {
    ($name:ident, $id:ty, $subject:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum $name {
            /// The internal identity, exactly as every existing caller spells it.
            Id($id),
            /// An exact canonical Jira key, resolved against confirmed bindings.
            Key(TrackerKey),
        }

        impl $name {
            /// Parse either accepted spelling.
            ///
            /// # Errors
            /// [`DomainError::Invalid`] when the text is neither a canonical v7
            /// UUID nor an exact canonical Jira key. The value is never echoed.
            pub fn parse(text: &str) -> DomainResult<Self> {
                if let Ok(id) = <$id>::parse(text) {
                    return Ok(Self::Id(id));
                }
                if let Ok(key) = TrackerKey::parse(text) {
                    return Ok(Self::Key(key));
                }
                Err(DomainError::invalid(
                    $subject,
                    "is neither a canonical identifier nor an exact canonical Jira key",
                ))
            }

            /// The already-resolved identity, when the caller supplied one.
            #[must_use]
            pub const fn id(&self) -> Option<&$id> {
                match self {
                    Self::Id(id) => Some(id),
                    Self::Key(_) => None,
                }
            }

            /// The key the caller supplied, when it was a key.
            #[must_use]
            pub const fn key(&self) -> Option<&TrackerKey> {
                match self {
                    Self::Key(key) => Some(key),
                    Self::Id(_) => None,
                }
            }
        }
    };
}

selector!(
    EpicSelector,
    MiniProjectId,
    "epic selector",
    "How a caller names one epic: by internal UUID, or by exact confirmed Jira key."
);
selector!(
    TaskSelector,
    TaskId,
    "task selector",
    "How a caller names one task: by internal UUID, or by exact confirmed Jira key."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uuid_selects_by_identity() {
        let text = "01a07722-c3ea-7300-9215-6f55309848f2";
        let selector = TaskSelector::parse(text).expect("a canonical task id parses");
        assert!(selector.id().is_some(), "{selector:?}");
        assert!(selector.key().is_none(), "{selector:?}");
    }

    #[test]
    fn a_canonical_key_selects_by_key() {
        let selector = TaskSelector::parse("ASMA-8119").expect("a canonical key parses");
        assert_eq!(
            selector.key().map(TrackerKey::as_str),
            Some("ASMA-8119"),
            "{selector:?}"
        );
        assert!(selector.id().is_none(), "{selector:?}");
    }

    #[test]
    fn the_two_grammars_do_not_collide() {
        // A UUID is never read as a key, and a key is never read as a UUID.
        let uuid = "01a07722-c3ea-7300-9215-6f55309848f2";
        assert!(TrackerKey::parse(uuid).is_err(), "a uuid is not a key");
        assert!(TaskId::parse("ASMA-8119").is_err(), "a key is not a uuid");
    }

    #[test]
    fn lowercase_and_malformed_spellings_are_refused() {
        // Case is never repaired, so a lowercase key stays a refusal.
        assert!(TaskSelector::parse("asma-8119").is_err());
        assert!(TaskSelector::parse("ASMA-0").is_err());
        assert!(TaskSelector::parse("ASMA").is_err());
        assert!(TaskSelector::parse("").is_err());
        assert!(EpicSelector::parse("not-an-identifier").is_err());
    }

    #[test]
    fn an_epic_selector_accepts_both_spellings() {
        assert!(EpicSelector::parse("01a0539a-51c9-7301-9bd7-26c09167b23e").is_ok());
        assert!(EpicSelector::parse("ASMA-8049").is_ok());
    }
}
