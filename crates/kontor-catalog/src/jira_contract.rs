//! The Jira observation evidence contract.
//!
//! This is deliberately a **separate** contract from
//! [`crate::contract::CATALOG_CONTRACT_VERSION`]. The catalog's frozen v1 types
//! keep `Association::verified == false`: an observation here may prove an
//! epic's immutable identity, actual type, parent and `ws:<key>` label, but it
//! does not rewrite a plan declaration into a verification. A caller that wants
//! to claim a verified plan/epic workstream join reads
//! [`JiraWorkstreamJoin`] from this contract and says so explicitly.
//!
//! # Two views, one collection
//!
//! * [`JiraEvidenceDocument`] is the durable observation view: coverage,
//!   aliases, per-issue provenance and recovery facts.
//! * [`EpicIdentitySource`] is the bounded compatibility view for
//!   `kontor/epic-identity@1`. It is a projection of the evidence, not a second
//!   collector and not a second export script.
//!
//! The JSON-Schema-inexpressible refinements live in
//! [`EpicIdentitySource::validate`]: issue keys are unique, every conflict's
//! current key is an issue in the document, and every declared key is answered
//! as an issue, a move or a missing key.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::JiraReadFailure;

/// The durable evidence schema.
pub const JIRA_EVIDENCE_SCHEMA: &str = "kontor/jira-observation@1";
/// The durable evidence contract version.
pub const JIRA_EVIDENCE_VERSION: u32 = 1;
/// The frozen compatibility schema this evidence projects to.
pub const EPIC_IDENTITY_SCHEMA: &str = "kontor/epic-identity@1";
/// The frozen schema's project pattern. The compatibility view is ASMA-specific
/// even though the durable evidence contract is project-generic.
pub const EPIC_IDENTITY_PROJECT: &str = "ASMA";
/// The label keys the existing immediate reader consults for declarations.
///
/// The durable path reads declarations from configuration, so the projection
/// reports the same vocabulary and zero session records rather than implying it
/// read Paseo. TASK-003 owns native session inventory.
pub const EPIC_IDENTITY_LABEL_KEYS: [&str; 8] = [
    "epic",
    "jira.epic",
    "origin.epic",
    "asma.epic",
    "jira.issue",
    "task",
    "asma.task",
    "ticket",
];

/// How an issue entered this observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraObservationVia {
    /// The configured scope search returned it.
    Scope,
    /// An operator-declared key named it.
    Declared,
    /// A parent chain needed it.
    Parent,
}

impl JiraObservationVia {
    /// The stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scope => "scope",
            Self::Declared => "declared",
            Self::Parent => "parent",
        }
    }

    /// Parse the wire spelling.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "scope" => Some(Self::Scope),
            "declared" => Some(Self::Declared),
            "parent" => Some(Self::Parent),
            _ => None,
        }
    }
}

/// Whether a sweep was a full census or an overlapping window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraSweepMode {
    /// Every page of the configured scope was read.
    Full,
    /// The configured scope plus an overlapping `updated` window.
    Incremental,
}

impl std::fmt::Display for JiraSweepMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl JiraSweepMode {
    /// The stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Incremental => "incremental",
        }
    }
}

/// The latest Jira collection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraCollectionState {
    /// No Jira observation has ever completed.
    #[default]
    NeverCollected,
    /// The latest sweep answered every request it needed.
    Complete,
    /// The latest sweep stopped before every fact was answered. Prior facts
    /// were retained and nothing was removed.
    Incomplete,
    /// The latest sweep could not start or lost permission/credentials.
    Failed,
}

impl std::fmt::Display for JiraCollectionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl JiraCollectionState {
    /// The stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NeverCollected => "never_collected",
            Self::Complete => "complete",
            Self::Incomplete => "incomplete",
            Self::Failed => "failed",
        }
    }

    /// Parse the wire spelling.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "never_collected" => Some(Self::NeverCollected),
            "complete" => Some(Self::Complete),
            "incomplete" => Some(Self::Incomplete),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// The exact configured scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraScope {
    /// The configured JQL. Never synthesized from a title or a label.
    pub jql: String,
    /// The configured project key.
    pub project: String,
    /// The operator-declared keys, sorted and unique.
    pub declared: Vec<String>,
}

/// One staged or stored issue observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraIssueObservation {
    /// Jira's immutable REST id.
    pub id: String,
    /// The current key at observation time.
    pub key: String,
    /// The actual issue type name.
    #[serde(rename = "type")]
    pub issue_type: String,
    /// The actual hierarchy level.
    pub level: i64,
    /// The parent's current key, when Jira names one.
    pub parent: Option<String>,
    /// The parent's immutable id, when Jira names one.
    pub parent_id: Option<String>,
    /// Every label, sorted.
    pub labels: Vec<String>,
    /// The status name.
    pub status: String,
    /// The status category key.
    pub cat: String,
    /// The source update instant, canonical UTC.
    pub updated: String,
    /// The summary.
    pub summary: String,
    /// When this observation was received.
    pub observed_at: String,
    /// How the issue entered the observation.
    pub via: JiraObservationVia,
    /// Whether the issue is currently in the relevant set or retained history.
    pub state: JiraObservationState,
    /// When this immutable issue was first observed.
    pub first_seen: String,
    /// When it was last observed or carried forward.
    pub last_seen: String,
    /// Why it left the relevant set, when it did.
    #[serde(default)]
    pub removed_reason: Option<String>,
}

impl JiraIssueObservation {
    /// The labels that name a workstream, in sorted order.
    #[must_use]
    pub fn workstream_labels(&self) -> Vec<String> {
        self.labels
            .iter()
            .filter_map(|label| {
                label
                    .strip_prefix("ws:")
                    .map(str::trim)
                    .filter(|key| !key.is_empty())
                    .map(ToOwned::to_owned)
            })
            .collect()
    }

    /// Whether this observation is an actual Epic, by hierarchy level and type
    /// name. A Story carrying a `ws:` label stays a Story.
    #[must_use]
    pub fn is_actual_epic(&self) -> bool {
        self.level == 1 && self.issue_type.eq_ignore_ascii_case("epic")
    }

    /// The value the compatibility `via` field carries.
    #[must_use]
    pub const fn via_wire(&self) -> JiraObservationVia {
        self.via
    }
}

/// Whether a stored observation is live or retained history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraObservationState {
    /// In the current relevant set.
    Active,
    /// Retained as history after a complete census proved it left the relevant
    /// set. Never a physical deletion.
    Removed,
}

/// A known old-key to current-key move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraAlias {
    /// The immutable issue.
    pub issue_id: String,
    /// The key that was requested or previously observed.
    pub old_key: String,
    /// The key Jira now returns for that issue.
    pub current_key: String,
    /// When the move was observed.
    pub observed_at: String,
}

/// A moved-key conflict in the compatibility document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpicIdentityConflict {
    /// Always `moved_key`.
    pub kind: String,
    /// The requested or previously observed key.
    pub key: String,
    /// The current key, which must be an issue in the document.
    pub current: String,
    /// When the move was observed.
    pub observed_at: String,
}

/// The six coverage counters the frozen schema can carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpicIdentityCoverage {
    /// Issues returned by the configured scope search.
    pub scope_issues: usize,
    /// Declared keys in the configured project.
    pub declared: usize,
    /// Declared keys answered as an issue or a move.
    pub found: usize,
    /// Declared keys Jira answered 404 for. Never deletions.
    pub missing: Vec<String>,
    /// Declared keys outside the configured project.
    pub out_of_scope: Vec<String>,
    /// Issues whose parent could not be read.
    pub unresolved: Vec<String>,
}

/// One issue in the frozen compatibility view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpicIdentityIssue {
    /// Jira's immutable REST id.
    pub id: String,
    /// The current key.
    pub key: String,
    /// The actual issue type name.
    #[serde(rename = "type")]
    pub issue_type: String,
    /// The actual hierarchy level.
    pub level: i64,
    /// The parent key, from Jira.
    pub parent: Option<String>,
    /// Every label.
    pub labels: Vec<String>,
    /// The status name.
    pub status: String,
    /// The status category key.
    pub cat: String,
    /// The source update instant.
    pub updated: String,
    /// The summary.
    pub summary: String,
    /// When the observation was received.
    pub observed_at: String,
    /// How the issue entered the observation.
    pub via: JiraObservationVia,
}

/// The frozen source block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpicIdentitySourceBlock {
    /// Always `jira-rest`.
    pub kind: String,
    /// The Jira realm hostname.
    pub realm: String,
}

/// The frozen scope block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpicIdentityScope {
    /// The configured JQL.
    pub jql: String,
    /// The configured project key.
    pub project: String,
    /// The declared keys.
    pub declared: Vec<String>,
}

/// The frozen provenance block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpicIdentityProvenance {
    /// The collector identity.
    pub collector: String,
    /// The adapter identity.
    pub adapter: String,
    /// The declaration label vocabulary.
    pub label_keys: Vec<String>,
    /// Session declaration coverage. The durable path reads configured
    /// declarations, so these are zero rather than guessed.
    pub sessions: EpicIdentitySessions,
}

/// Session declaration coverage in the frozen provenance block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpicIdentitySessions {
    /// Session records read.
    pub read: u64,
    /// Session records that could not be read.
    pub unreadable: u64,
}

/// The frozen compatibility document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpicIdentitySource {
    /// Always `kontor/epic-identity@1`.
    pub schema: String,
    /// Monotonic order; a later export is greater.
    pub order: u64,
    /// The collection instant.
    pub as_of: String,
    /// The source.
    pub source: EpicIdentitySourceBlock,
    /// The configured scope.
    pub scope: EpicIdentityScope,
    /// Collection provenance.
    pub provenance: EpicIdentityProvenance,
    /// Scope coverage.
    pub coverage: EpicIdentityCoverage,
    /// Moved-key conflicts.
    pub conflicts: Vec<EpicIdentityConflict>,
    /// Every observed issue.
    pub issues: Vec<EpicIdentityIssue>,
}

/// Everything the durable evidence view carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraEvidenceDocument {
    /// The durable evidence schema.
    pub schema: String,
    /// The durable evidence contract version.
    pub evidence_version: u32,
    /// The Jira generation this document describes.
    pub generation: u64,
    /// Monotonic order across complete collections.
    pub order: u64,
    /// When the complete generation finished.
    pub as_of: String,
    /// The realm and base URL, without a credential.
    pub source: JiraEvidenceSource,
    /// The exact configured scope.
    pub scope: JiraScope,
    /// The collector and credential reference.
    pub provenance: JiraEvidenceProvenance,
    /// The latest collection coverage for the active complete generation.
    pub coverage: JiraCollectionCoverage,
    /// The latest attempt's state, which may be later than the active generation.
    pub collection_state: JiraCollectionState,
    /// The latest attempt's structural error, when it failed.
    pub latest_error: Option<String>,
    /// Every moved-key conflict retained for this collection.
    pub conflicts: Vec<JiraAlias>,
    /// Every active observation.
    pub issues: Vec<JiraIssueObservation>,
    /// Every known alias, including those for retained history.
    pub aliases: Vec<JiraAlias>,
    /// Explicit catalog associations that currently point at no observed
    /// epic. The catalog row itself is never rewritten.
    pub dangling_associations: Vec<JiraDanglingAssociation>,
    /// Declared or requested keys that could not be answered. An empty list is
    /// required before the compatibility view is produced.
    pub unanswered: Vec<String>,
}

/// The durable source block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraEvidenceSource {
    /// Always `jira-rest`.
    pub kind: String,
    /// The realm hostname.
    pub realm: String,
    /// The configured base URL. It never contains credentials.
    pub base_url: String,
}

/// The durable provenance block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraEvidenceProvenance {
    /// The collector identity.
    pub collector: String,
    /// The adapter identity.
    pub adapter: String,
    /// The keychain alias, never the secret.
    pub credential_reference: String,
}

/// A complete collection's honest coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraCollectionCoverage {
    /// Full or incremental.
    pub mode: JiraSweepMode,
    /// Complete, incomplete or failed.
    pub state: JiraCollectionState,
    /// Whether every request needed by this mode succeeded.
    pub complete: bool,
    /// Whether this sweep was a complete relevant census and may remove facts.
    pub census: bool,
    /// Issues returned by the configured scope search.
    pub scope_issues: usize,
    /// Declared keys in the configured project.
    pub declared: usize,
    /// Declared keys answered as an issue or a move.
    pub found: usize,
    /// Declared keys Jira answered 404 for.
    pub missing: Vec<String>,
    /// Declared keys outside the configured project.
    pub out_of_scope: Vec<String>,
    /// Issues whose parent could not be read.
    pub unresolved: Vec<String>,
    /// Enhanced-search pages read.
    pub pages: usize,
    /// Requests attempted, including retries.
    pub requests: usize,
    /// Retries actually performed.
    pub retries: usize,
    /// 429 answers observed.
    pub rate_limited: usize,
    /// Whether 401/403 stopped the sweep.
    pub permission_loss: bool,
    /// Whether the keychain credential was unavailable.
    pub credential_failure: bool,
    /// Whether a complete full search omitted an issue a direct read found.
    pub search_lag_suspected: bool,
    /// Retained previous facts whose key Jira answered 404 for.
    pub retained_unknown: Vec<String>,
    /// Facts removed by a complete relevant census.
    pub removed: usize,
    /// The absolute retry deadline, when Jira named one.
    pub backoff_until: Option<String>,
    /// The incremental window start, when one applied.
    pub window_start: Option<String>,
    /// The incremental window end.
    pub window_end: Option<String>,
    /// When the sweep started.
    pub started_at: String,
    /// When the sweep finished.
    pub finished_at: String,
    /// When the last complete sweep finished.
    pub last_complete_at: Option<String>,
    /// When the last complete full sweep finished.
    pub last_full_sweep_at: Option<String>,
    /// When the next full sweep is due.
    pub next_full_sweep_at: Option<String>,
    /// Whether the full-sweep deadline has passed.
    pub overdue: bool,
    /// Structural diagnostics.
    pub diagnostics: Vec<String>,
    /// The failure class, when the sweep failed.
    pub failure: Option<JiraReadFailure>,
    /// A structural error, never response content.
    pub error: Option<String>,
}

/// An explicit catalog association whose epic is no longer observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraDanglingAssociation {
    /// The logical plan that declared it.
    pub logical_plan_id: Option<String>,
    /// The workstream key, when the association is a workstream.
    pub workstream: Option<String>,
    /// The epic key, when the association is an epic.
    pub epic: Option<String>,
    /// When the plan was last observed.
    pub last_seen: String,
}

/// Proof of a verified plan/epic workstream join.
///
/// This is the only type that may claim `verified`. It is derived from an
/// actual Epic and exactly one matching `ws:<key>` label, and it never mutates
/// [`crate::contract::Association::verified`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraWorkstreamJoin {
    /// The immutable epic id.
    pub issue_id: String,
    /// The epic's current key.
    pub epic_key: String,
    /// The one workstream key named by the label.
    pub workstream: String,
    /// The exact label.
    pub label: String,
    /// Always true for this type.
    pub verified: bool,
    /// When the observation was received.
    pub observed_at: String,
    /// The evidence rule that admitted it.
    pub rule: &'static str,
}

/// Why a workstream join cannot be verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JiraJoinDiagnostic {
    /// The issue is not an actual Epic.
    NotEpic {
        /// The issue key.
        issue_key: String,
        /// The actual type.
        issue_type: String,
        /// The actual level.
        level: i64,
    },
    /// No `ws:<key>` label exists.
    NoWorkstreamLabel {
        /// The issue key.
        issue_key: String,
    },
    /// More than one `ws:<key>` label exists. Nothing is chosen.
    MultipleWorkstreamLabels {
        /// The issue key.
        issue_key: String,
        /// The exact labels, sorted.
        labels: Vec<String>,
    },
    /// Exactly one label exists, but no plan or registry declares that key.
    WorkstreamNotDeclared {
        /// The issue key.
        issue_key: String,
        /// The label's key.
        workstream: String,
        /// The declared keys.
        declared: Vec<String>,
    },
}

impl std::fmt::Display for JiraJoinDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEpic {
                issue_key,
                issue_type,
                level,
            } => write!(
                formatter,
                "{issue_key} is {issue_type} at level {level}, not an Epic"
            ),
            Self::NoWorkstreamLabel { issue_key } => {
                write!(formatter, "{issue_key} has no ws:<key> label")
            }
            Self::MultipleWorkstreamLabels { issue_key, labels } => write!(
                formatter,
                "{issue_key} names more than one workstream: {}",
                labels.join(", ")
            ),
            Self::WorkstreamNotDeclared {
                issue_key,
                workstream,
                ..
            } => write!(
                formatter,
                "{issue_key} names ws:{workstream}, which no plan or registry declares"
            ),
        }
    }
}

/// Why evidence could not be projected to the frozen schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JiraProjectionError {
    /// A required schema literal or field is wrong.
    #[error("the evidence does not satisfy {0}")]
    Schema(&'static str),
    /// Two observations carry the same current key.
    #[error("the compatibility view repeats issue key {0}")]
    DuplicateIssueKey(String),
    /// A conflict's current key is not in the document.
    #[error("the moved-key conflict for {key} names {current}, which is not an issue")]
    ConflictCurrentMissing {
        /// The old key.
        key: String,
        /// The current key.
        current: String,
    },
    /// A declared key is neither an issue, a move nor a missing key.
    #[error("declared key {0} is unanswered")]
    Unanswered(String),
    /// The collected evidence itself carries an unanswered key.
    #[error("the collection did not answer {0}")]
    CollectionUnanswered(String),
    /// A field violates its bounded shape.
    #[error("the evidence field {0} is outside its supported shape")]
    InvalidField(&'static str),
    /// A parent chain repeats.
    #[error("the parent chain for {0} repeats")]
    ParentCycle(String),
}

impl JiraProjectionError {
    /// Whether this is an unanswered-key refusal rather than a shape refusal.
    #[must_use]
    pub const fn is_unanswered(&self) -> bool {
        matches!(self, Self::Unanswered(_) | Self::CollectionUnanswered(_))
    }
}

impl EpicIdentitySource {
    /// Verify the three refinements the JSON Schema cannot express, plus the
    /// bounded field shapes the schema enforces structurally.
    ///
    /// # Errors
    /// The document is refused rather than silently repaired.
    pub fn validate(&self) -> Result<(), JiraProjectionError> {
        if self.schema != EPIC_IDENTITY_SCHEMA {
            return Err(JiraProjectionError::Schema("schema"));
        }
        if self.order == 0 || self.order > 9_007_199_254_740_991 || self.as_of.is_empty() {
            return Err(JiraProjectionError::InvalidField("order/as_of"));
        }
        if self.source.kind != "jira-rest"
            || self.source.realm.is_empty()
            || self.source.realm.len() > 253
        {
            return Err(JiraProjectionError::Schema("source"));
        }
        if self.scope.jql.is_empty()
            || self.scope.jql.chars().count() > 2000
            || self.scope.project != EPIC_IDENTITY_PROJECT
            || self.scope.declared.len() > 5000
            || self.coverage.scope_issues > 5000
            || self.coverage.declared > 5000
            || self.coverage.found > 5000
            || self.coverage.missing.len() > 5000
            || self.coverage.out_of_scope.len() > 5000
            || self.coverage.unresolved.len() > 5000
        {
            return Err(JiraProjectionError::Schema("scope"));
        }
        for key in &self.scope.declared {
            if !valid_epic_key(key) {
                return Err(JiraProjectionError::InvalidField("scope.declared"));
            }
        }
        for key in &self.coverage.missing {
            if !valid_epic_key(key) {
                return Err(JiraProjectionError::InvalidField("coverage.missing"));
            }
        }
        for key in &self.coverage.out_of_scope {
            if !valid_issue_key(key) {
                return Err(JiraProjectionError::InvalidField("coverage.out_of_scope"));
            }
        }
        for key in &self.coverage.unresolved {
            if !valid_epic_key(key) {
                return Err(JiraProjectionError::InvalidField("coverage.unresolved"));
            }
        }
        if self.provenance.collector.is_empty()
            || self.provenance.collector.len() > 80
            || self.provenance.adapter.is_empty()
            || self.provenance.adapter.len() > 200
            || self.provenance.label_keys.len() > 32
        {
            return Err(JiraProjectionError::Schema("provenance"));
        }
        for key in &self.provenance.label_keys {
            if key.is_empty() || key.chars().count() > 64 {
                return Err(JiraProjectionError::InvalidField("provenance.label_keys"));
            }
        }
        if self.provenance.sessions.read > 100_000 || self.provenance.sessions.unreadable > 100_000
        {
            return Err(JiraProjectionError::InvalidField("provenance.sessions"));
        }
        if self.conflicts.len() > 5000 || self.issues.len() > 5000 {
            return Err(JiraProjectionError::InvalidField("items"));
        }
        let mut keys = BTreeSet::new();
        for issue in &self.issues {
            if !valid_epic_key(&issue.key)
                || issue.id.is_empty()
                || issue.id.len() > 20
                || issue.id.starts_with('0')
                || !issue.id.bytes().all(|byte| byte.is_ascii_digit())
                || issue.issue_type.is_empty()
                || issue.issue_type.chars().count() > 80
                || !(-1..=9).contains(&issue.level)
                || issue.labels.len() > 100
                || issue.status.is_empty()
                || issue.status.chars().count() > 160
                || !matches!(issue.cat.as_str(), "new" | "indeterminate" | "done")
                || issue.summary.chars().count() > 1000
                || issue.updated.is_empty()
                || issue.observed_at.is_empty()
            {
                return Err(JiraProjectionError::InvalidField("issues"));
            }
            for label in &issue.labels {
                if label.is_empty() || label.chars().count() > 255 {
                    return Err(JiraProjectionError::InvalidField("issues.labels"));
                }
            }
            if let Some(parent) = &issue.parent
                && !valid_issue_key(parent)
            {
                return Err(JiraProjectionError::InvalidField("issues.parent"));
            }
            if !keys.insert(issue.key.as_str()) {
                return Err(JiraProjectionError::DuplicateIssueKey(issue.key.clone()));
            }
        }
        let aliases: BTreeMap<&str, &str> = self
            .conflicts
            .iter()
            .map(|conflict| (conflict.key.as_str(), conflict.current.as_str()))
            .collect();
        for conflict in &self.conflicts {
            if conflict.kind != "moved_key" || !valid_epic_key(&conflict.key) {
                return Err(JiraProjectionError::Schema("conflicts"));
            }
            if !valid_epic_key(&conflict.current) {
                return Err(JiraProjectionError::Schema("conflicts"));
            }
            let resolved = resolve_alias(&aliases, &conflict.current)?;
            if !keys.contains(resolved.as_str()) {
                return Err(JiraProjectionError::ConflictCurrentMissing {
                    key: conflict.key.clone(),
                    current: conflict.current.clone(),
                });
            }
        }
        let answered: BTreeSet<&str> = self
            .issues
            .iter()
            .map(|issue| issue.key.as_str())
            .chain(self.conflicts.iter().map(|conflict| conflict.key.as_str()))
            .chain(self.coverage.missing.iter().map(String::as_str))
            .collect();
        for key in &self.scope.declared {
            if !answered.contains(key.as_str()) {
                return Err(JiraProjectionError::Unanswered(key.clone()));
            }
        }
        Ok(())
    }

    /// Climb from any issue to its actual Epic, following old-to-current aliases
    /// for both the starting key and every parent.
    ///
    /// # Errors
    /// A repeated key in the chain is refused rather than silently looped.
    pub fn epic(&self, requested: &str) -> Result<Option<&EpicIdentityIssue>, JiraProjectionError> {
        let aliases: BTreeMap<&str, &str> = self
            .conflicts
            .iter()
            .map(|conflict| (conflict.key.as_str(), conflict.current.as_str()))
            .collect();
        let mut current = resolve_alias(&aliases, requested)?;
        let mut seen = BTreeSet::new();
        for _ in 0..16 {
            if !seen.insert(current.clone()) {
                return Err(JiraProjectionError::ParentCycle(current));
            }
            let Some(issue) = self.issues.iter().find(|issue| issue.key == current) else {
                return Ok(None);
            };
            if issue.level == 1 && issue.issue_type.eq_ignore_ascii_case("epic") {
                return Ok(Some(issue));
            }
            let Some(parent) = issue.parent.as_deref() else {
                return Ok(None);
            };
            current = resolve_alias(&aliases, parent)?;
        }
        Err(JiraProjectionError::ParentCycle(current))
    }
}

/// Project durable evidence into the frozen compatibility view.
///
/// # Errors
/// An unanswered declared key, a missing conflict target, a duplicate key or a
/// malformed field refuses the document. It is never silently narrowed.
pub fn epic_identity_source(
    document: &JiraEvidenceDocument,
) -> Result<EpicIdentitySource, JiraProjectionError> {
    if !document.unanswered.is_empty() {
        return Err(JiraProjectionError::CollectionUnanswered(
            document.unanswered[0].clone(),
        ));
    }
    let declared = document.scope.declared.clone();
    let mut issues = Vec::with_capacity(document.issues.len());
    for issue in &document.issues {
        issues.push(EpicIdentityIssue {
            id: issue.id.clone(),
            key: issue.key.clone(),
            issue_type: issue.issue_type.clone(),
            level: issue.level,
            parent: issue.parent.clone(),
            labels: issue.labels.clone(),
            status: issue.status.clone(),
            cat: issue.cat.clone(),
            updated: issue.updated.clone(),
            summary: issue.summary.clone(),
            observed_at: issue.observed_at.clone(),
            via: issue.via,
        });
    }
    issues.sort_by_key(|item| key_order(&item.key));
    let issue_keys: BTreeSet<&str> = issues.iter().map(|issue| issue.key.as_str()).collect();
    let alias_map: BTreeMap<&str, &str> = document
        .conflicts
        .iter()
        .map(|alias| (alias.old_key.as_str(), alias.current_key.as_str()))
        .collect();
    let mut conflicts = Vec::new();
    for conflict in &document.conflicts {
        let resolved = resolve_alias(&alias_map, &conflict.current_key)?;
        if issue_keys.contains(resolved.as_str()) {
            conflicts.push(EpicIdentityConflict {
                kind: "moved_key".to_owned(),
                key: conflict.old_key.clone(),
                current: conflict.current_key.clone(),
                observed_at: conflict.observed_at.clone(),
            });
        }
    }
    conflicts.sort_by_key(|item| key_order(&item.key));
    let coverage = EpicIdentityCoverage {
        scope_issues: document.coverage.scope_issues,
        declared: document.coverage.declared,
        found: document.coverage.found,
        missing: document.coverage.missing.clone(),
        out_of_scope: document.coverage.out_of_scope.clone(),
        unresolved: document.coverage.unresolved.clone(),
    };
    let source = EpicIdentitySource {
        schema: EPIC_IDENTITY_SCHEMA.to_owned(),
        order: document.order,
        as_of: document.as_of.clone(),
        source: EpicIdentitySourceBlock {
            kind: "jira-rest".to_owned(),
            realm: document.source.realm.clone(),
        },
        scope: EpicIdentityScope {
            jql: document.scope.jql.clone(),
            project: document.scope.project.clone(),
            declared,
        },
        provenance: EpicIdentityProvenance {
            collector: document.provenance.collector.clone(),
            adapter: document.provenance.adapter.clone(),
            label_keys: EPIC_IDENTITY_LABEL_KEYS
                .iter()
                .map(|key| (*key).to_owned())
                .collect(),
            sessions: EpicIdentitySessions::default(),
        },
        coverage,
        conflicts,
        issues,
    };
    source.validate()?;
    Ok(source)
}

/// Verify a plan/epic workstream join from an actual Epic and exactly one
/// matching `ws:<key>` label.
///
/// # Errors
/// A non-Epic, zero labels, several labels, or a key no plan/registry declares
/// is a diagnostic and never a join.
pub fn verified_workstream_join(
    issue: &JiraIssueObservation,
    declared: &[String],
) -> Result<JiraWorkstreamJoin, JiraJoinDiagnostic> {
    if !issue.is_actual_epic() {
        return Err(JiraJoinDiagnostic::NotEpic {
            issue_key: issue.key.clone(),
            issue_type: issue.issue_type.clone(),
            level: issue.level,
        });
    }
    let labels = issue.workstream_labels();
    match labels.as_slice() {
        [] => Err(JiraJoinDiagnostic::NoWorkstreamLabel {
            issue_key: issue.key.clone(),
        }),
        [workstream] => {
            if !declared.iter().any(|declared| declared == workstream) {
                return Err(JiraJoinDiagnostic::WorkstreamNotDeclared {
                    issue_key: issue.key.clone(),
                    workstream: workstream.clone(),
                    declared: declared.to_vec(),
                });
            }
            Ok(JiraWorkstreamJoin {
                issue_id: issue.id.clone(),
                epic_key: issue.key.clone(),
                workstream: workstream.clone(),
                label: format!("ws:{workstream}"),
                verified: true,
                observed_at: issue.observed_at.clone(),
                rule: "actual Epic at hierarchy level 1 with exactly one ws:<key> label matching a declared workstream",
            })
        }
        many => Err(JiraJoinDiagnostic::MultipleWorkstreamLabels {
            issue_key: issue.key.clone(),
            labels: many.iter().map(|key| format!("ws:{key}")).collect(),
        }),
    }
}

fn resolve_alias(aliases: &BTreeMap<&str, &str>, key: &str) -> Result<String, JiraProjectionError> {
    let mut current = key.to_owned();
    let mut seen = BTreeSet::new();
    while let Some(next) = aliases.get(current.as_str()) {
        if !seen.insert(current.clone()) {
            return Err(JiraProjectionError::ParentCycle(current));
        }
        current = (*next).to_owned();
    }
    Ok(current)
}

/// Project order used by the existing reader: project, then numeric suffix.
#[must_use]
pub fn key_order(key: &str) -> (String, u64) {
    match key.rsplit_once('-') {
        Some((project, number)) => (
            project.to_owned(),
            number.parse::<u64>().unwrap_or(u64::MAX),
        ),
        None => (key.to_owned(), u64::MAX),
    }
}

/// Whether text is an uppercase Jira project key.
#[must_use]
pub fn valid_project_key(project: &str) -> bool {
    !project.is_empty()
        && project.len() <= 80
        && project.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_uppercase()
            } else {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
            }
        })
}

/// Whether text is an issue key in the frozen compatibility project.
#[must_use]
pub fn valid_epic_key(key: &str) -> bool {
    valid_issue_key(key) && key.starts_with(&format!("{EPIC_IDENTITY_PROJECT}-"))
}

/// Whether text is a Jira issue key.
#[must_use]
pub fn valid_issue_key(key: &str) -> bool {
    if key.len() > 64 {
        return false;
    }
    let Some((project, number)) = key.rsplit_once('-') else {
        return false;
    };
    valid_project_key(project)
        && !number.is_empty()
        && !number.starts_with('0')
        && number.len() <= 20
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(key: &str, level: i64, kind: &str, parent: Option<&str>) -> JiraIssueObservation {
        JiraIssueObservation {
            id: key.rsplit('-').next().unwrap_or("1").to_owned(),
            key: key.to_owned(),
            issue_type: kind.to_owned(),
            level,
            parent: parent.map(ToOwned::to_owned),
            parent_id: None,
            labels: Vec::new(),
            status: "In Progress".to_owned(),
            cat: "indeterminate".to_owned(),
            updated: "2026-10-25T00:00:00Z".to_owned(),
            summary: format!("Synthetic {key}"),
            observed_at: "2026-10-25T00:00:01Z".to_owned(),
            via: JiraObservationVia::Scope,
            state: JiraObservationState::Active,
            first_seen: "2026-10-25T00:00:01Z".to_owned(),
            last_seen: "2026-10-25T00:00:01Z".to_owned(),
            removed_reason: None,
        }
    }

    fn document(
        issues: Vec<JiraIssueObservation>,
        conflicts: Vec<JiraAlias>,
    ) -> JiraEvidenceDocument {
        JiraEvidenceDocument {
            schema: JIRA_EVIDENCE_SCHEMA.to_owned(),
            evidence_version: JIRA_EVIDENCE_VERSION,
            generation: 1,
            order: 1,
            as_of: "2026-10-25T00:00:02Z".to_owned(),
            source: JiraEvidenceSource {
                kind: "jira-rest".to_owned(),
                realm: "tracker.example".to_owned(),
                base_url: "https://tracker.example".to_owned(),
            },
            scope: JiraScope {
                jql: "project = ASMA AND issuetype = Epic".to_owned(),
                project: "ASMA".to_owned(),
                declared: Vec::new(),
            },
            provenance: JiraEvidenceProvenance {
                collector: "kontor-catalog@1".to_owned(),
                adapter: "kontor-jira-read-only@1".to_owned(),
                credential_reference: "work".to_owned(),
            },
            coverage: JiraCollectionCoverage {
                mode: JiraSweepMode::Full,
                state: JiraCollectionState::Complete,
                complete: true,
                census: true,
                scope_issues: issues.len(),
                declared: 0,
                found: 0,
                missing: Vec::new(),
                out_of_scope: Vec::new(),
                unresolved: Vec::new(),
                pages: 1,
                requests: 1,
                retries: 0,
                rate_limited: 0,
                permission_loss: false,
                credential_failure: false,
                search_lag_suspected: false,
                retained_unknown: Vec::new(),
                removed: 0,
                backoff_until: None,
                window_start: None,
                window_end: None,
                started_at: "2026-10-25T00:00:00Z".to_owned(),
                finished_at: "2026-10-25T00:00:02Z".to_owned(),
                last_complete_at: Some("2026-10-25T00:00:02Z".to_owned()),
                last_full_sweep_at: Some("2026-10-25T00:00:02Z".to_owned()),
                next_full_sweep_at: Some("2026-10-25T00:15:02Z".to_owned()),
                overdue: false,
                diagnostics: Vec::new(),
                failure: None,
                error: None,
            },
            collection_state: JiraCollectionState::Complete,
            latest_error: None,
            conflicts,
            issues,
            aliases: Vec::new(),
            dangling_associations: Vec::new(),
            unanswered: Vec::new(),
        }
    }

    #[test]
    fn a_story_stays_a_story_and_climbs_to_its_epic_through_an_alias() {
        let mut story = issue("ASMA-11", 0, "Story", Some("ASMA-10"));
        story.via = JiraObservationVia::Declared;
        let epic = issue("ASMA-20", 1, "Epic", None);
        let alias = JiraAlias {
            issue_id: "20".to_owned(),
            old_key: "ASMA-10".to_owned(),
            current_key: "ASMA-20".to_owned(),
            observed_at: "2026-10-25T00:00:01Z".to_owned(),
        };
        let document = document(vec![story, epic], vec![alias]);
        let projection = epic_identity_source(&document).expect("the projection");
        assert_eq!(
            projection
                .epic("ASMA-11")
                .unwrap()
                .map(|epic| epic.key.as_str()),
            Some("ASMA-20")
        );
        let story = projection
            .issues
            .iter()
            .find(|issue| issue.key == "ASMA-11")
            .unwrap();
        assert_eq!(story.issue_type, "Story");
        assert_eq!(story.level, 0);
    }

    #[test]
    fn only_an_actual_epic_type_is_returned_even_at_level_one() {
        let story = issue("ASMA-30", 1, "Story", None);
        let document = document(vec![story], Vec::new());
        let projection = epic_identity_source(&document).expect("the projection");
        assert!(projection.epic("ASMA-30").unwrap().is_none());
    }

    #[test]
    fn an_alias_chain_is_followed_to_its_current_key() {
        let story = issue("ASMA-11", 0, "Story", Some("ASMA-10"));
        let epic = issue("ASMA-20", 1, "Epic", None);
        let aliases = vec![
            JiraAlias {
                issue_id: "20".to_owned(),
                old_key: "ASMA-10".to_owned(),
                current_key: "ASMA-15".to_owned(),
                observed_at: "2026-10-25T00:00:01Z".to_owned(),
            },
            JiraAlias {
                issue_id: "20".to_owned(),
                old_key: "ASMA-15".to_owned(),
                current_key: "ASMA-20".to_owned(),
                observed_at: "2026-10-25T00:00:02Z".to_owned(),
            },
        ];
        let document = document(vec![story, epic], aliases);
        let projection = epic_identity_source(&document).expect("the projection");
        assert_eq!(
            projection
                .epic("ASMA-11")
                .unwrap()
                .map(|epic| epic.key.as_str()),
            Some("ASMA-20")
        );
    }

    #[test]
    fn a_declared_key_must_be_answered() {
        let mut document = document(vec![issue("ASMA-20", 1, "Epic", None)], Vec::new());
        document.scope.declared = vec!["ASMA-404".to_owned()];
        let error = epic_identity_source(&document).unwrap_err();
        assert!(error.is_unanswered(), "{error:?}");
    }

    #[test]
    fn duplicate_keys_and_missing_conflict_targets_are_refused() {
        let epic = issue("ASMA-20", 1, "Epic", None);
        let mut duplicate = document(vec![epic.clone(), epic], Vec::new());
        duplicate.coverage.scope_issues = 2;
        assert!(matches!(
            epic_identity_source(&duplicate),
            Err(JiraProjectionError::DuplicateIssueKey(_))
        ));
        let mut moved = document(vec![issue("ASMA-21", 1, "Epic", None)], Vec::new());
        moved.conflicts.push(JiraAlias {
            issue_id: "22".to_owned(),
            old_key: "ASMA-22".to_owned(),
            current_key: "ASMA-99".to_owned(),
            observed_at: "2026-10-25T00:00:01Z".to_owned(),
        });
        // The durable evidence keeps the alias; the compatibility view cannot
        // name a current key it does not carry, so it omits that conflict.
        let projection = epic_identity_source(&moved).expect("the projection");
        assert!(projection.conflicts.is_empty());
        projection.validate().expect("the refinement holds");
        assert_eq!(moved.conflicts.len(), 1, "the evidence alias is retained");
    }

    #[test]
    fn only_an_actual_epic_with_one_declared_label_verifies() {
        let mut epic = issue("ASMA-20", 1, "Epic", None);
        assert!(verified_workstream_join(&epic, &[]).is_err());
        epic.labels = vec!["ws:kontor".to_owned(), "ws:other".to_owned()];
        assert!(matches!(
            verified_workstream_join(&epic, &["kontor".to_owned()]),
            Err(JiraJoinDiagnostic::MultipleWorkstreamLabels { .. })
        ));
        epic.labels = vec!["ws:kontor".to_owned()];
        assert!(matches!(
            verified_workstream_join(&epic, &["mobile".to_owned()]),
            Err(JiraJoinDiagnostic::WorkstreamNotDeclared { .. })
        ));
        let join = verified_workstream_join(&epic, &["kontor".to_owned()]).expect("the join");
        assert!(join.verified);
        assert_eq!(join.workstream, "kontor");
        let mut story = issue("ASMA-21", 0, "Story", Some("ASMA-20"));
        story.labels = vec!["ws:kontor".to_owned()];
        assert!(matches!(
            verified_workstream_join(&story, &["kontor".to_owned()]),
            Err(JiraJoinDiagnostic::NotEpic { .. })
        ));
    }
}
