//! Specification-owned native container and seat display names.
//!
//! Names are rendered from one immutable topology revision plus durable token
//! values supplied by the daemon.  This module deliberately knows nothing about
//! topology kind semantics, runtime adapters, paths, descriptions or ids: a
//! template can use only the closed vocabulary below, and a missing value is a
//! refusal rather than an invitation to infer one.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::id::ExternalName;
use crate::{DomainError, DomainResult};

/// The exact byte sequence joining adjacent name-template segments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameSeparator(String);

impl NameSeparator {
    /// The ASMA Operational v1 separator (`SPACE`, U+2022, `SPACE`).
    pub const DEFAULT: &'static str = " • ";

    /// Parse an exact separator without trimming or normalization.
    ///
    /// # Errors
    /// Refuses empty/whitespace-only text and control characters. Both the
    /// historical U+2022 BULLET and the canonical U+00B7 MIDDLE DOT are valid
    /// specification-owned separators; old pinned revisions keep their bytes.
    pub fn parse(value: &str) -> DomainResult<Self> {
        if value.is_empty() || !value.chars().any(|character| !character.is_whitespace()) {
            return Err(DomainError::invalid(
                "NameSeparator",
                "must contain at least one non-whitespace scalar",
            ));
        }
        if value.chars().any(char::is_control) {
            return Err(DomainError::invalid(
                "NameSeparator",
                "must not contain control characters",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    /// Exact preserved UTF-8 text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for NameSeparator {
    fn default() -> Self {
        Self(Self::DEFAULT.to_owned())
    }
}

impl Serialize for NameSeparator {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NameSeparator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

/// Optional intake-time AI label stored byte-for-byte for later revisions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AiShortName(String);

impl AiShortName {
    /// Validate a trimmed two-keyword value separated by one ASCII space.
    ///
    /// # Errors
    /// Refuses any other word count/spacing, controls, separator glyphs and
    /// values longer than 64 Unicode scalar values.
    pub fn parse(value: &str) -> DomainResult<Self> {
        if value.chars().count() > 64
            || value.trim() != value
            || value.chars().any(char::is_control)
            || value.contains(['\u{2022}', '\u{00b7}'])
        {
            return Err(DomainError::invalid(
                "AiShortName",
                "must be a bounded trimmed two-keyword value without separator glyphs",
            ));
        }
        let mut words = value.split(' ');
        if !matches!((words.next(), words.next(), words.next()), (Some(first), Some(second), None) if !first.is_empty() && !second.is_empty())
        {
            return Err(DomainError::invalid(
                "AiShortName",
                "must contain exactly two keywords separated by one ASCII space",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    /// Exact preserved UTF-8 text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for AiShortName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AiShortName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

crate::closed_enum! {
    /// Closed values a published native-name template may request.
    NativeNameToken, "NativeNameToken" {
        /// Configured local prefix for one native container kind.
        Prefix => "PREFIX",
        /// Jira-derived item code for the owning epic.
        EpicItemCode => "EPIC_ITEM_CODE",
        /// Jira-derived item code for the owning task.
        TaskItemCode => "TASK_ITEM_CODE",
        /// Subject-selected epic or task item code for a consultation.
        ScopeItemCode => "SCOPE_ITEM_CODE",
        /// Explicit bounded consultation topic.
        Topic => "TOPIC",
        /// Exact registered local professional role code.
        RoleCode => "ROLE_CODE",
        /// Exact display label declared for one local seat slot.
        SlotDisplayName => "SLOT_DISPLAY_NAME",
        /// Topology kind for a container; snapshotted role code for a seat.
        AreaCode => "AREA_CODE",
        /// The single durable Jira identity applicable to the owning scope.
        JiraCode => "JIRA_CODE",
        /// Explicit epic backlog code or task short code.
        KontorBacklogCode => "KONTOR_BACKLOG_CODE",
        /// Jira-derived item code from the epic namespace and confirmed suffix.
        ItemCode => "ITEM_CODE",
        /// Immutable intake-time AI label.
        AiShortName => "AI_SHORT_NAME",
    }
}

/// One ordered part of a native display-name template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NativeNameSegment {
    /// A value resolved from durable typed state.
    Token(NativeNameToken),
    /// Specification-owned literal text.
    Literal(ExternalName),
}

/// Typed templates plus a read-only representation of pre-v47 strings.
///
/// The legacy variant exists so old custom specifications remain readable and
/// exportable. [`Self::validate`] always refuses it, so it can never be newly
/// published, materialized or used for repair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum NativeNameTemplate {
    /// Current typed template.
    Typed {
        /// Ordered segments joined by the specification separator.
        segments: Vec<NativeNameSegment>,
    },
    /// Pre-v47 opaque display text (read compatibility only).
    Legacy(ExternalName),
}

impl NativeNameTemplate {
    /// Construct and validate one typed template.
    ///
    /// # Errors
    /// As [`Self::validate`].
    pub fn from_segments(segments: Vec<NativeNameSegment>) -> DomainResult<Self> {
        let template = Self::Typed { segments };
        template.validate()?;
        Ok(template)
    }

    /// Borrow typed segments. Legacy strings have none.
    #[must_use]
    pub fn segments(&self) -> Option<&[NativeNameSegment]> {
        match self {
            Self::Typed { segments } => Some(segments),
            Self::Legacy(_) => None,
        }
    }

    /// Validate one publishable template.
    ///
    /// # Errors
    /// Refuses legacy/empty templates, duplicate token segments, and literals
    /// carrying either separator glyph or a control character.
    pub fn validate(&self) -> DomainResult<()> {
        let Self::Typed { segments } = self else {
            return Err(DomainError::invalid(
                "NativeNameTemplate",
                "legacy string templates are read-only and cannot be published",
            ));
        };
        if segments.is_empty() {
            return Err(DomainError::invalid(
                "NativeNameTemplate",
                "must contain at least one segment",
            ));
        }
        let mut tokens = BTreeSet::new();
        for segment in segments {
            match segment {
                NativeNameSegment::Token(token) if !tokens.insert(*token) => {
                    return Err(DomainError::invalid(
                        "NativeNameTemplate",
                        "must not repeat a token segment",
                    ));
                }
                NativeNameSegment::Literal(value)
                    if value.as_str().contains('\u{00b7}')
                        || value.as_str().contains('\u{2022}') =>
                {
                    return Err(DomainError::invalid(
                        "NativeNameTemplate",
                        "literal segments must not contain separator glyphs",
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Render exact bytes from explicit values.
    ///
    /// # Errors
    /// Refuses a legacy template or any absent required token. No fallback or
    /// derived value is available to this function.
    pub fn render(
        &self,
        separator: &NameSeparator,
        values: &NativeNameValues,
    ) -> DomainResult<ExternalName> {
        self.validate()?;
        let Self::Typed { segments } = self else {
            unreachable!("validation admitted only a typed template")
        };
        let rendered = segments
            .iter()
            .map(|segment| match segment {
                NativeNameSegment::Literal(value) => Ok(value.as_str()),
                NativeNameSegment::Token(token) => {
                    let value = values.require(*token)?;
                    if value.contains(separator.as_str())
                        || value.contains(['\u{2022}', '\u{00b7}'])
                    {
                        return Err(DomainError::invalid(
                            "NativeNameValues",
                            "a rendered token must not contain a separator glyph",
                        ));
                    }
                    Ok(value)
                }
            })
            .collect::<DomainResult<Vec<_>>>()?
            .join(separator.as_str());
        ExternalName::parse(&rendered)
    }
}

/// Explicit values available to one pure rendering operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeNameValues(BTreeMap<NativeNameToken, String>);

impl NativeNameValues {
    /// Empty values, useful for incrementally adding only authorized facts.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `PREFIX`.
    #[must_use]
    pub fn with_prefix(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::Prefix, value)
    }

    /// Add `EPIC_ITEM_CODE`.
    #[must_use]
    pub fn with_epic_item_code(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::EpicItemCode, value)
    }

    /// Add `TASK_ITEM_CODE`.
    #[must_use]
    pub fn with_task_item_code(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::TaskItemCode, value)
    }

    /// Add `SCOPE_ITEM_CODE`.
    #[must_use]
    pub fn with_scope_item_code(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::ScopeItemCode, value)
    }

    /// Add `TOPIC`.
    #[must_use]
    pub fn with_topic(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::Topic, value)
    }

    /// Add `ROLE_CODE`.
    #[must_use]
    pub fn with_role_code(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::RoleCode, value)
    }

    /// Add `SLOT_DISPLAY_NAME`.
    #[must_use]
    pub fn with_slot_display_name(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::SlotDisplayName, value)
    }

    /// Add `AREA_CODE`.
    #[must_use]
    pub fn with_area_code(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::AreaCode, value)
    }

    /// Add `JIRA_CODE`.
    #[must_use]
    pub fn with_jira_code(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::JiraCode, value)
    }

    /// Add `KONTOR_BACKLOG_CODE`.
    #[must_use]
    pub fn with_kontor_backlog_code(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::KontorBacklogCode, value)
    }

    /// Add a derived `ITEM_CODE` projection.
    #[must_use]
    pub fn with_item_code(self, value: impl Into<String>) -> Self {
        self.with(NativeNameToken::ItemCode, value)
    }

    /// Add a validated `AI_SHORT_NAME`.
    #[must_use]
    pub fn with_ai_short_name(self, value: &AiShortName) -> Self {
        self.with(NativeNameToken::AiShortName, value.as_str())
    }

    fn with(mut self, token: NativeNameToken, value: impl Into<String>) -> Self {
        self.0.insert(token, value.into());
        self
    }

    fn require(&self, token: NativeNameToken) -> DomainResult<&str> {
        self.0
            .get(&token)
            .map(String::as_str)
            .ok_or_else(|| match token {
                NativeNameToken::Prefix => {
                    DomainError::invalid("NativeNameTemplate", "missing PREFIX")
                }
                NativeNameToken::EpicItemCode => {
                    DomainError::invalid("NativeNameTemplate", "missing EPIC_ITEM_CODE")
                }
                NativeNameToken::TaskItemCode => {
                    DomainError::invalid("NativeNameTemplate", "missing TASK_ITEM_CODE")
                }
                NativeNameToken::ScopeItemCode => {
                    DomainError::invalid("NativeNameTemplate", "missing SCOPE_ITEM_CODE")
                }
                NativeNameToken::Topic => {
                    DomainError::invalid("NativeNameTemplate", "missing TOPIC")
                }
                NativeNameToken::RoleCode => {
                    DomainError::invalid("NativeNameTemplate", "missing ROLE_CODE")
                }
                NativeNameToken::SlotDisplayName => {
                    DomainError::invalid("NativeNameTemplate", "missing SLOT_DISPLAY_NAME")
                }
                NativeNameToken::AreaCode => {
                    DomainError::invalid("NativeNameTemplate", "missing AREA_CODE")
                }
                NativeNameToken::JiraCode => {
                    DomainError::invalid("NativeNameTemplate", "missing JIRA_CODE")
                }
                NativeNameToken::KontorBacklogCode => {
                    DomainError::invalid("NativeNameTemplate", "missing KONTOR_BACKLOG_CODE")
                }
                NativeNameToken::ItemCode => {
                    DomainError::invalid("NativeNameTemplate", "missing ITEM_CODE")
                }
                NativeNameToken::AiShortName => {
                    DomainError::invalid("NativeNameTemplate", "missing AI_SHORT_NAME")
                }
            })
    }
}
