//! Durable Kontor backlog identities and Jira-derived display projections.

use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::id::{ExternalId, ExternalName};
use crate::{DomainError, DomainResult};

/// One immutable epic namespace inside a Kontor project.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EpicBacklogCode(String);

impl EpicBacklogCode {
    /// Parse a manually selected canonical backlog code.
    ///
    /// # Errors
    /// Refuses anything except 2–32 uppercase ASCII letters and digits, and
    /// refuses an all-numeric value so a Jira number cannot become a namespace.
    pub fn parse(value: impl AsRef<str>) -> DomainResult<Self> {
        let value = value.as_ref();
        if !(2..=32).contains(&value.len()) {
            return Err(DomainError::invalid(
                "epic backlog code",
                "must contain between 2 and 32 ASCII characters",
            ));
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Err(DomainError::invalid(
                "epic backlog code",
                "must contain only uppercase ASCII letters and digits",
            ));
        }
        if value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(DomainError::invalid(
                "epic backlog code",
                "must not be a Jira number",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    /// Allocate the first deterministic code not present in `used`.
    ///
    /// The title initials are tried first. Collisions append unused title
    /// characters in column-major order, then the smallest numeric ordinal.
    /// Comparisons are case-insensitive so legacy non-canonical rows cannot
    /// collide with a new canonical assignment.
    ///
    /// # Errors
    /// Refuses a title with fewer than two usable ASCII-alphanumeric bytes.
    pub fn allocate<'a>(
        title: &ExternalName,
        used: impl IntoIterator<Item = &'a str>,
    ) -> DomainResult<Self> {
        let used = used
            .into_iter()
            .map(str::to_ascii_uppercase)
            .collect::<BTreeSet<_>>();
        let words = title
            .as_str()
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(|word| {
                word.bytes()
                    .map(|byte| byte.to_ascii_uppercase())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let usable = words.iter().map(Vec::len).sum::<usize>();
        if usable < 2 {
            return Err(DomainError::invalid(
                "epic backlog code",
                "the title has fewer than two usable ASCII-alphanumeric characters",
            ));
        }

        let mut candidate = words
            .iter()
            .filter_map(|word| word.first().copied())
            .take(32)
            .collect::<Vec<_>>();
        if candidate.len() >= 2 {
            let text =
                String::from_utf8(candidate.clone()).expect("the allocator emits only ASCII bytes");
            if !used.contains(&text) {
                return Self::parse(text);
            }
        }
        let max_width = words.iter().map(Vec::len).max().unwrap_or_default();
        for column in 1..max_width {
            for word in &words {
                let Some(byte) = word.get(column).copied() else {
                    continue;
                };
                if candidate.len() < 32 {
                    candidate.push(byte);
                }
                if candidate.len() >= 2 {
                    let text = String::from_utf8(candidate.clone())
                        .expect("the allocator emits only ASCII bytes");
                    if !used.contains(&text) {
                        return Self::parse(text);
                    }
                }
            }
        }
        if candidate.len() >= 2 {
            let text =
                String::from_utf8(candidate.clone()).expect("the allocator emits only ASCII bytes");
            if !used.contains(&text) {
                return Self::parse(text);
            }
        }

        for ordinal in 2_u64.. {
            let suffix = ordinal.to_string();
            let keep = 32_usize.saturating_sub(suffix.len());
            if keep < 2 {
                break;
            }
            let mut numbered = candidate[..candidate.len().min(keep)].to_vec();
            numbered.extend_from_slice(suffix.as_bytes());
            let text = String::from_utf8(numbered).expect("the allocator emits only ASCII bytes");
            if !used.contains(&text) {
                return Self::parse(text);
            }
        }
        Err(DomainError::invalid(
            "epic backlog code",
            "the deterministic candidate space is exhausted",
        ))
    }

    /// Borrow the canonical spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EpicBacklogCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for EpicBacklogCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EpicBacklogCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

/// Exact spelling of one backlog code imported before canonical enforcement.
///
/// This type exists only as a comparison and evidence value for the bounded
/// legacy-correction path. New assignments remain [`EpicBacklogCode`] and
/// therefore cannot use a historical non-canonical spelling.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LegacyEpicBacklogCode(ExternalId);

impl LegacyEpicBacklogCode {
    /// Parse one historical code that entered through the legacy external-id
    /// field.
    ///
    /// # Errors
    /// Uses the original external-id bound: the value must be non-empty, at
    /// most 256 characters, whitespace/control-free and non-sensitive.
    pub fn parse(value: impl AsRef<str>) -> DomainResult<Self> {
        ExternalId::parse(value.as_ref()).map(Self)
    }

    /// Borrow the exact historical spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for LegacyEpicBacklogCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for LegacyEpicBacklogCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LegacyEpicBacklogCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

/// One exact confirmed Jira issue key, in its canonical published spelling.
///
/// A confirmed binding is the public identity of an epic or task. This type
/// admits only the canonical `<PROJECT>-<positive decimal>` spelling, so a
/// title, a bare number, a legacy item code or a UUID can never reach a
/// rendered native name by being mistaken for a key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfirmedJiraKey(String);

impl ConfirmedJiraKey {
    /// Admit one externally read-back Jira key by its exact spelling.
    ///
    /// # Errors
    /// Refuses anything except a canonical `<PROJECT>-<positive decimal>` key,
    /// including zero and decimal suffixes with leading zeroes.
    pub fn parse(confirmed_jira_key: &ExternalId) -> DomainResult<Self> {
        let (project_key, number) =
            confirmed_jira_key
                .as_str()
                .rsplit_once('-')
                .ok_or_else(|| {
                    DomainError::invalid(
                        "confirmed Jira issue key",
                        "must end with a canonical positive decimal suffix",
                    )
                })?;
        if project_key.is_empty()
            || !project_key.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_uppercase()
                    || (index > 0 && byte.is_ascii_digit())
                    || (index > 0 && byte == b'-')
            })
        {
            return Err(DomainError::invalid(
                "confirmed Jira issue key",
                "must have a canonical uppercase project key",
            ));
        }
        if number.is_empty()
            || number.starts_with('0')
            || !number.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(DomainError::invalid(
                "confirmed Jira issue key",
                "must end with a canonical positive decimal suffix",
            ));
        }
        Ok(Self(confirmed_jira_key.as_str().to_owned()))
    }

    /// Borrow the exact confirmed key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The canonical decimal suffix, without its project key.
    #[must_use]
    pub fn number(&self) -> &str {
        self.0
            .rsplit_once('-')
            .expect("a parsed key always carries its canonical suffix")
            .1
    }
}

impl std::fmt::Display for ConfirmedJiraKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ConfirmedJiraKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// Display-only projection of an epic namespace and a confirmed Jira number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JiraItemCode(String);

impl JiraItemCode {
    /// Derive `<epic backlog code>-<canonical Jira decimal suffix>`.
    ///
    /// The complete Jira key remains the binding authority; this value carries
    /// no project key and cannot be used to reconstruct that binding.
    ///
    /// # Errors
    /// As [`ConfirmedJiraKey::parse`]: refuses anything except a canonical
    /// `<PROJECT>-<positive decimal>` key.
    pub fn derive(
        backlog_code: &EpicBacklogCode,
        confirmed_jira_key: &ExternalId,
    ) -> DomainResult<Self> {
        Ok(Self::from_confirmed(
            backlog_code,
            &ConfirmedJiraKey::parse(confirmed_jira_key)?,
        ))
    }

    /// Project an already-admitted confirmed key into its display item code.
    ///
    /// The key was canonical when it was parsed, so this cannot fail: one
    /// confirmed-binding admission serves both projections.
    #[must_use]
    pub fn from_confirmed(backlog_code: &EpicBacklogCode, confirmed: &ConfirmedJiraKey) -> Self {
        Self(format!("{}-{}", backlog_code.as_str(), confirmed.number()))
    }

    /// Borrow the derived display spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for JiraItemCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for JiraItemCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}
