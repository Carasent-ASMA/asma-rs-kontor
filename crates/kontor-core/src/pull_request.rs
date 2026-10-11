//! One pull request a PR desk reviews (ASMA-8453).
//!
//! A pull request is identified by its forge repository and its number, and
//! by nothing else. Two pull requests of one Jira ticket are two subjects, and
//! the same number in two repositories is two subjects. Neither half is ever
//! read back out of a display title: the `<repo>#<n>` a review renders is
//! derived from this value, never the other way round.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{DomainError, DomainResult};

/// The largest pull-request number a subject may carry: SQLite's signed
/// 64-bit integer, which is where the number is stored.
pub const MAX_PULL_REQUEST_NUMBER: u64 = i64::MAX as u64;

/// One forge repository, as `owner/name`.
///
/// The owner is ASCII letters, digits and inner hyphens; the name is ASCII
/// letters, digits, `.`, `_` and `-`, and is neither `.` nor `..`. Spelling is
/// exact: no case folding, trimming or `.git` suffix stripping, so a
/// repository is authorized only under the spelling its mapping holds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForgeRepository(String);

impl ForgeRepository {
    /// Parse one exact `owner/name`.
    ///
    /// # Errors
    /// Refuses anything but exactly one `/` between a valid owner of at most
    /// 39 characters and a valid name of at most 100.
    pub fn parse(text: &str) -> DomainResult<Self> {
        let refused = || {
            DomainError::invalid(
                "ForgeRepository",
                "must be one exact owner/name with a GitHub-shaped owner and name",
            )
        };
        let (owner, name) = text.split_once('/').ok_or_else(refused)?;
        let owner_ok = (1..=39).contains(&owner.len())
            && !owner.starts_with('-')
            && !owner.ends_with('-')
            && owner
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
        let name_ok = (1..=100).contains(&name.len())
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        if !owner_ok || !name_ok {
            return Err(refused());
        }
        Ok(Self(text.to_owned()))
    }

    /// The exact `owner/name`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The repository's name without its owner, as a review title shows it.
    #[must_use]
    pub fn name(&self) -> &str {
        self.0
            .split_once('/')
            .map_or(self.0.as_str(), |(_, name)| name)
    }
}

impl std::fmt::Display for ForgeRepository {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ForgeRepository {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ForgeRepository {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(D::Error::custom)
    }
}

/// One pull request: its repository and its number, both part of its
/// identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PullRequestRef {
    repository: ForgeRepository,
    number: u64,
}

impl PullRequestRef {
    /// One pull request of one repository.
    ///
    /// # Errors
    /// Refuses number zero and a number past [`MAX_PULL_REQUEST_NUMBER`].
    pub fn new(repository: ForgeRepository, number: u64) -> DomainResult<Self> {
        if number == 0 || number > MAX_PULL_REQUEST_NUMBER {
            return Err(DomainError::invalid(
                "PullRequestRef",
                "the number must be a positive pull-request number",
            ));
        }
        Ok(Self { repository, number })
    }

    /// The repository.
    #[must_use]
    pub const fn repository(&self) -> &ForgeRepository {
        &self.repository
    }

    /// The number inside its repository.
    #[must_use]
    pub const fn number(&self) -> u64 {
        self.number
    }

    /// The `<repo>#<n>` a review title renders for it.
    #[must_use]
    pub fn title_value(&self) -> String {
        format!("{}#{}", self.repository.name(), self.number)
    }
}

impl<'de> Deserialize<'de> for PullRequestRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            repository: ForgeRepository,
            number: u64,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.repository, wire.number).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_is_one_exact_owner_and_name() {
        let repository = ForgeRepository::parse("Carasent-ASMA/asma-rs-kontor").expect("valid");
        assert_eq!(repository.name(), "asma-rs-kontor");
        for refused in [
            "",
            "asma-rs-kontor",
            "/asma-rs-kontor",
            "Carasent-ASMA/",
            "Carasent-ASMA/a/b",
            "-owner/name",
            "owner-/name",
            "owner/..",
            "owner/.",
            "owner/na me",
            "own_er/name",
            "owner/name#1",
            " owner/name",
        ] {
            assert!(ForgeRepository::parse(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn a_pull_request_renders_its_repository_name_and_number() {
        let repository = ForgeRepository::parse("Carasent-ASMA/asma-modules").expect("valid");
        let pull_request = PullRequestRef::new(repository.clone(), 2985).expect("valid");
        assert_eq!(pull_request.title_value(), "asma-modules#2985");
        assert!(PullRequestRef::new(repository.clone(), 0).is_err());
        assert!(PullRequestRef::new(repository, MAX_PULL_REQUEST_NUMBER + 1).is_err());
    }

    #[test]
    fn a_pull_request_round_trips_and_refuses_number_zero_on_the_wire() {
        let parsed: PullRequestRef =
            serde_json::from_str(r#"{"repository":"Carasent-ASMA/asma-modules","number":7}"#)
                .expect("valid");
        assert_eq!(
            serde_json::to_string(&parsed).expect("serializes"),
            r#"{"repository":"Carasent-ASMA/asma-modules","number":7}"#
        );
        assert!(
            serde_json::from_str::<PullRequestRef>(
                r#"{"repository":"Carasent-ASMA/asma-modules","number":0}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<PullRequestRef>(
                r#"{"repository":"Carasent-ASMA/asma-modules","number":7,"title":"x"}"#
            )
            .is_err()
        );
    }
}
