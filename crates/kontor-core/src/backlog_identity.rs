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
    /// Refuses anything except a canonical `<PROJECT>-<positive decimal>` key,
    /// including zero and decimal suffixes with leading zeroes.
    pub fn derive(
        backlog_code: &EpicBacklogCode,
        confirmed_jira_key: &ExternalId,
    ) -> DomainResult<Self> {
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
        Ok(Self(format!("{}-{number}", backlog_code.as_str())))
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
