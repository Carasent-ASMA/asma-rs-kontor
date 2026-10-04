//! Versioned experience documents and authority-free retrieval contracts.
#![allow(missing_docs)]

use crate::id::{CanonicalDocument, ContentHash, ProjectId, Timestamp, reject_sensitive_text};
use crate::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

pub const MAX_RECALL_ITEMS: usize = 8;
pub const MAX_RECALL_BYTES: usize = 32768;
pub const MAX_CANDIDATES: usize = 64;
pub const MAX_EXPERIENCE_BYTES: usize = 65536;

closed_enum! { ExperienceKind, "ExperienceKind" {
    Experience => "experience", Lesson => "lesson", MentalModel => "mental_model"
}}
closed_enum! { EvidenceConfidence, "EvidenceConfidence" { Inferred => "inferred", Observed => "observed" }}
closed_enum! { #[derive(Default)] ProjectionPolicy, "ProjectionPolicy" { #[default] LocalOnly => "local_only", ProviderEligible => "provider_eligible" }}
closed_enum! { OutcomeKind, "OutcomeKind" { Success => "success", Failure => "failure", Mixed => "mixed" }}
closed_enum! { RetrievalMode, "RetrievalMode" { Semantic => "semantic", LexicalDegraded => "lexical_degraded", None => "none" }}
closed_enum! { DegradedReason, "DegradedReason" {
    Absent => "absent", Stale => "stale", Timeout => "timeout", Unavailable => "unavailable",
    Malformed => "malformed", Empty => "empty", NoEligibleCandidates => "no_eligible_candidates"
}}
closed_enum! { MemoryRefusal, "MemoryRefusal" {
    InvalidExperience => "invalid_experience", UnresolvedEvidence => "unresolved_evidence",
    FrozenPayloadPurged => "frozen_payload_purged", FrozenPayloadMismatch => "frozen_payload_mismatch",
    BindingConflict => "binding_conflict", CandidateLimit => "candidate_limit",
    ProjectionConflict => "projection_conflict", ProjectionUnavailable => "projection_unavailable"
}}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceOutcome {
    pub kind: OutcomeKind,
    pub summary: String,
}

/// Receipts and revisions resolve in the owning project. Artifacts are immutable
/// relative evidence locators plus a required digest, never files to open or URLs to fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceRef {
    Receipt {
        receipt_id: String,
        content_hash: ContentHash,
    },
    MemoryRevision {
        project_id: ProjectId,
        item_id: String,
        revision_id: String,
        content_hash: ContentHash,
    },
    Artifact {
        locator: String,
        content_hash: ContentHash,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceMemoryV1 {
    pub schema_version: u32,
    pub document_type: String,
    pub kind: ExperienceKind,
    pub situation: String,
    pub intent: String,
    pub actions: Vec<String>,
    pub outcome: ExperienceOutcome,
    pub went_well: Vec<String>,
    pub went_wrong: Vec<String>,
    pub lesson: String,
    pub future_cues: Vec<String>,
    pub avoid: Vec<String>,
    pub domains: Vec<String>,
    pub occurred_at: Timestamp,
    pub confidence: EvidenceConfidence,
    #[serde(default)]
    pub projection_policy: ProjectionPolicy,
    pub evidence_refs: Vec<EvidenceRef>,
}

impl ExperienceMemoryV1 {
    pub fn from_document(document: &CanonicalDocument) -> DomainResult<Self> {
        let experience: Self = document
            .deserialize()
            .map_err(|_| DomainError::invalid("ExperienceMemoryV1", "invalid_experience"))?;
        experience.validate()?;
        Ok(experience)
    }
    pub fn canonical(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }
    pub fn validate(&self) -> DomainResult<()> {
        if self.schema_version != 1 || self.document_type != "experience_memory" {
            return Err(DomainError::invalid(
                "ExperienceMemoryV1",
                "invalid_experience",
            ));
        }
        for (path, value) in [
            ("situation", &self.situation),
            ("intent", &self.intent),
            ("lesson", &self.lesson),
            ("outcome.summary", &self.outcome.summary),
        ] {
            text(path, value, 4096)?;
        }
        for (path, values, min) in [
            ("actions", &self.actions, 1),
            ("went_well", &self.went_well, 0),
            ("went_wrong", &self.went_wrong, 0),
            ("future_cues", &self.future_cues, 1),
            ("avoid", &self.avoid, 0),
            ("domains", &self.domains, 1),
        ] {
            list(path, values, min)?;
        }
        if self.evidence_refs.is_empty() || self.evidence_refs.len() > 16 {
            return Err(DomainError::invalid_at(
                "ExperienceMemoryV1",
                "evidence_refs",
                "requires 1..16 references",
            ));
        }
        for evidence in &self.evidence_refs {
            match evidence {
                EvidenceRef::Receipt { receipt_id, .. } => {
                    text("evidence_refs.receipt_id", receipt_id, 128)?;
                    uuid::Uuid::parse_str(receipt_id).map_err(|_| {
                        DomainError::invalid_at(
                            "ExperienceMemoryV1",
                            "evidence_refs.receipt_id",
                            "requires a UUID",
                        )
                    })?;
                }
                EvidenceRef::MemoryRevision {
                    item_id,
                    revision_id,
                    ..
                } => {
                    text("evidence_refs.item_id", item_id, 128)?;
                    text("evidence_refs.revision_id", revision_id, 128)?;
                    uuid::Uuid::parse_str(revision_id).map_err(|_| {
                        DomainError::invalid_at(
                            "ExperienceMemoryV1",
                            "evidence_refs.revision_id",
                            "requires a UUID",
                        )
                    })?;
                }
                EvidenceRef::Artifact { locator, .. } => {
                    text("evidence_refs.locator", locator, 1024)?;
                    if locator.starts_with('/')
                        || locator.contains(':')
                        || locator.contains('\\')
                        || locator
                            .split('/')
                            .any(|part| part.is_empty() || part == "." || part == "..")
                    {
                        return Err(DomainError::invalid_at(
                            "ExperienceMemoryV1",
                            "evidence_refs.locator",
                            "requires a relative artifact locator",
                        ));
                    }
                }
            }
        }
        // Scan all nested values through the canonical scanner, including evidence
        // keys. Do not create a second, weaker sensitive-material validator.
        let doc = CanonicalDocument::from_serializable(self)?;
        if doc.json().len() > MAX_EXPERIENCE_BYTES {
            return Err(DomainError::invalid(
                "ExperienceMemoryV1",
                "exceeds 65536 canonical bytes",
            ));
        }
        Ok(())
    }
}

pub fn is_recall_eligible(document: &CanonicalDocument) -> bool {
    ExperienceMemoryV1::from_document(document).is_ok()
}

pub(crate) fn text(path: &'static str, value: &str, max: usize) -> DomainResult<()> {
    if value.trim().is_empty() || value.len() > max {
        return Err(DomainError::invalid_at(
            "ExperienceMemoryV1",
            path,
            "text is empty or exceeds its UTF-8 byte bound",
        ));
    }
    reject_sensitive_text(path, value)
}
fn list(path: &'static str, values: &[String], min: usize) -> DomainResult<()> {
    if values.len() < min || values.len() > 16 {
        return Err(DomainError::invalid_at(
            "ExperienceMemoryV1",
            path,
            "list exceeds its cardinality bounds",
        ));
    }
    for value in values {
        text(path, value, 2048)?;
    }
    Ok(())
}

/// Constructed by the domain service from durable task/work-profile state.
/// Public recall accepts task identity only, never this query text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallIntent {
    pub schema_version: u32,
    pub task_id: String,
    pub task_title: String,
    pub module: Option<String>,
    pub declared_scope: Vec<String>,
    pub phase: String,
}
impl RecallIntent {
    pub fn canonical(&self) -> DomainResult<CanonicalDocument> {
        if self.schema_version != 1 {
            return Err(DomainError::invalid("RecallIntent", "unsupported version"));
        }
        text("task_id", &self.task_id, 128)?;
        text("task_title", &self.task_title, 4096)?;
        text("phase", &self.phase, 128)?;
        if let Some(module) = &self.module {
            text("module", module, 1024)?;
        }
        list("declared_scope", &self.declared_scope, 0)?;
        CanonicalDocument::from_serializable(self)
    }
    /// At most 24 quoted terms; punctuation can never become FTS syntax.
    pub fn lexical_query(&self) -> DomainResult<String> {
        self.canonical()?;
        let source = std::iter::once(self.task_title.as_str())
            .chain(self.module.as_deref())
            .chain(self.declared_scope.iter().map(String::as_str))
            .chain(std::iter::once(self.phase.as_str()))
            .collect::<Vec<_>>()
            .join(" ");
        let mut terms = std::collections::BTreeSet::new();
        for word in source
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.len() >= 2)
            .take(64)
        {
            terms.insert(word.chars().take(64).collect::<String>().to_lowercase());
            if terms.len() == 24 {
                break;
            }
        }
        Ok(terms
            .into_iter()
            .map(|term| format!("\"{term}\""))
            .collect::<Vec<_>>()
            .join(" OR "))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCandidate {
    pub project_id: ProjectId,
    pub item_id: String,
    pub revision_id: String,
    pub content_hash: ContentHash,
    pub score: f64,
}
impl MemoryCandidate {
    pub fn validate(&self) -> DomainResult<()> {
        text("candidate.item_id", &self.item_id, 128)?;
        text("candidate.revision_id", &self.revision_id, 128)?;
        if !self.score.is_finite() {
            return Err(DomainError::invalid(
                "MemoryCandidate",
                "score must be finite",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryIdentity {
    pub project_id: ProjectId,
    pub item_id: String,
    pub revision_id: String,
    pub content_hash: ContentHash,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallExclusions {
    pub invalid: u32,
    pub duplicate: u32,
    pub budget: u32,
    pub item_limit: u32,
}

/// No evidence, transcript or approval fields cross the provider boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionEntry {
    pub identity: MemoryIdentity,
    pub cues: Vec<String>,
    pub lesson: String,
}
