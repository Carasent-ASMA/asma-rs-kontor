//! Completion-gate evidence and escalation invariants.
//!
//! The completion scheduler consumes these pure decisions. This module knows
//! neither a bundled profile name nor a runtime: it checks the evidence a
//! pinned profile requires and makes an incomplete `needs_human` record
//! unrepresentable, including after deserialization.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::{BoundedText, ContentHash, ExternalName, OpenQuestionId, TaskId};
use kontor_core::open_question::{OpenQuestionStatus, OpenQuestionSummary};
use kontor_core::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

/// One ticket's declared completion contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketRequirement {
    /// The ticket.
    pub task_id: TaskId,
    /// Goal keys that must be certified.
    pub goals: BTreeSet<ExternalName>,
    /// Evidence keys that must be present.
    pub evidence: BTreeSet<ExternalName>,
}

/// The completion evidence currently recorded for one ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketEvidence {
    /// The ticket.
    pub task_id: TaskId,
    /// Goal keys with durable completion evidence.
    pub goals: BTreeSet<ExternalName>,
    /// Evidence keys with durable artifacts.
    pub evidence: BTreeSet<ExternalName>,
}

/// Why the all-tickets gate is still closed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TicketGateBlocker {
    /// No evidence record exists for the ticket.
    MissingTicket(TaskId),
    /// A declared goal has not been certified.
    MissingGoal {
        /// The ticket.
        task_id: TaskId,
        /// The missing goal key.
        goal: ExternalName,
    },
    /// A declared artifact/evidence key is absent.
    MissingEvidence {
        /// The ticket.
        task_id: TaskId,
        /// The missing evidence key.
        evidence: ExternalName,
    },
}

/// Check the all-tickets completion gate in stable ticket/key order.
///
/// # Errors
/// Refuses duplicate declarations/evidence and evidence for an undeclared
/// ticket rather than guessing which record is authoritative.
pub fn ticket_gate_blockers(
    requirements: &[TicketRequirement],
    evidence: &[TicketEvidence],
) -> DomainResult<Vec<TicketGateBlocker>> {
    let mut declared = BTreeMap::new();
    for requirement in requirements {
        if declared.insert(requirement.task_id, requirement).is_some() {
            return Err(DomainError::invalid(
                "completion ticket requirements",
                "a ticket may be declared only once",
            ));
        }
    }

    let mut recorded = BTreeMap::new();
    for record in evidence {
        if !declared.contains_key(&record.task_id) {
            return Err(DomainError::invalid(
                "completion ticket evidence",
                "evidence must name a declared ticket",
            ));
        }
        if recorded.insert(record.task_id, record).is_some() {
            return Err(DomainError::invalid(
                "completion ticket evidence",
                "a ticket may have only one current evidence record",
            ));
        }
    }

    let mut blockers = Vec::new();
    for (task_id, requirement) in declared {
        let Some(record) = recorded.get(&task_id) else {
            blockers.push(TicketGateBlocker::MissingTicket(task_id));
            continue;
        };
        blockers.extend(
            requirement
                .goals
                .difference(&record.goals)
                .cloned()
                .map(|goal| TicketGateBlocker::MissingGoal { task_id, goal }),
        );
        blockers.extend(
            requirement
                .evidence
                .difference(&record.evidence)
                .cloned()
                .map(|evidence| TicketGateBlocker::MissingEvidence { task_id, evidence }),
        );
    }
    Ok(blockers)
}

/// The mandatory closeout receipts in their execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseoutRequirement {
    /// The approved integration outcome was merged.
    Merge,
    /// The release was confirmed.
    Release,
    /// Delivered module/service revisions were inventoried.
    VersionInventory,
    /// The final summary was recorded.
    Summary,
    /// Stakeholders were notified.
    Notification,
    /// The epic resources were archived.
    Archive,
}

impl CloseoutRequirement {
    /// Every prerequisite required by the Operational profile.
    pub const ALL: [Self; 6] = [
        Self::Merge,
        Self::Release,
        Self::VersionInventory,
        Self::Summary,
        Self::Notification,
        Self::Archive,
    ];

    /// Stable projection spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge_receipt",
            Self::Release => "release_receipt",
            Self::VersionInventory => "version_inventory",
            Self::Summary => "summary_receipt",
            Self::Notification => "notification_receipt",
            Self::Archive => "archive_receipt",
        }
    }
}

/// Evidence accumulated during closeout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseoutEvidence {
    /// Merge confirmation.
    pub merge_receipt: Option<ContentHash>,
    /// Release confirmation.
    pub release_receipt: Option<ContentHash>,
    /// Delivered module/service revisions, keyed by module/service name.
    pub delivered_versions: BTreeMap<ExternalName, ExternalName>,
    /// Final summary receipt.
    pub summary_receipt: Option<ContentHash>,
    /// Notification receipt.
    pub notification_receipt: Option<ContentHash>,
    /// Archive receipt.
    pub archive_receipt: Option<ContentHash>,
}

/// Return the required closeout evidence that is still absent.
#[must_use]
pub fn closeout_blockers(evidence: &CloseoutEvidence) -> Vec<CloseoutRequirement> {
    let mut blockers = Vec::new();
    if evidence.merge_receipt.is_none() {
        blockers.push(CloseoutRequirement::Merge);
    }
    if evidence.release_receipt.is_none() {
        blockers.push(CloseoutRequirement::Release);
    }
    if evidence.delivered_versions.is_empty() {
        blockers.push(CloseoutRequirement::VersionInventory);
    }
    if evidence.summary_receipt.is_none() {
        blockers.push(CloseoutRequirement::Summary);
    }
    if evidence.notification_receipt.is_none() {
        blockers.push(CloseoutRequirement::Notification);
    }
    if evidence.archive_receipt.is_none() {
        blockers.push(CloseoutRequirement::Archive);
    }
    blockers
}

/// One durable step in the deliberation path exhausted before human input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliberationStep {
    /// The role(s) that acted.
    pub role: ExternalName,
    /// The consultation or recovery mechanism used.
    pub consultation: ExternalName,
    /// The completion/remediation round.
    pub round: u8,
    /// Its outcome.
    pub outcome: ExternalName,
}

/// Mandatory context for a `needs_human` completion state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UncheckedNeedsHuman")]
pub struct NeedsHumanPayload {
    recommended_resolution: ExternalName,
    tried_deliberation_path: Vec<DeliberationStep>,
}

#[derive(Deserialize)]
struct UncheckedNeedsHuman {
    recommended_resolution: ExternalName,
    tried_deliberation_path: Vec<DeliberationStep>,
}

impl NeedsHumanPayload {
    /// Build a complete escalation record.
    ///
    /// # Errors
    /// Refuses an empty deliberation path; the recommendation is already a
    /// validated non-empty [`ExternalName`].
    pub fn new(
        recommended_resolution: ExternalName,
        tried_deliberation_path: Vec<DeliberationStep>,
    ) -> DomainResult<Self> {
        if tried_deliberation_path.is_empty() {
            return Err(DomainError::invalid(
                "needs_human",
                "must name the roles, consultations, and rounds already tried",
            ));
        }
        Ok(Self {
            recommended_resolution,
            tried_deliberation_path,
        })
    }

    /// The recommended human resolution.
    #[must_use]
    pub const fn recommended_resolution(&self) -> &ExternalName {
        &self.recommended_resolution
    }

    /// The exhausted deliberation path.
    #[must_use]
    pub fn tried_deliberation_path(&self) -> &[DeliberationStep] {
        &self.tried_deliberation_path
    }
}

impl TryFrom<UncheckedNeedsHuman> for NeedsHumanPayload {
    type Error = DomainError;

    fn try_from(value: UncheckedNeedsHuman) -> Result<Self, Self::Error> {
        Self::new(value.recommended_resolution, value.tried_deliberation_path)
    }
}

// ---------------------------------------------------------------------------
// The open-question gate (OP-REQ-038)
// ---------------------------------------------------------------------------

/// Why an unresolved ambiguity is holding completion open.
///
/// The identity and the subject are both carried, because a blocker naming only
/// a UUID tells whoever has to go and close the question nothing about what it
/// is. Both are data rather than a rendered sentence, for the same reason every
/// other blocker in this module is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OpenQuestionBlocker {
    /// Raised and never dispositioned.
    Undispositioned {
        /// The question.
        question_id: OpenQuestionId,
        /// What it is about.
        subject: BoundedText,
    },
    /// Deferred, and the trigger it was parked on has since fired.
    Reopened {
        /// The question.
        question_id: OpenQuestionId,
        /// What it is about.
        subject: BoundedText,
    },
}

impl OpenQuestionBlocker {
    /// The question this blocker names.
    #[must_use]
    pub const fn question_id(&self) -> OpenQuestionId {
        match self {
            Self::Undispositioned { question_id, .. } | Self::Reopened { question_id, .. } => {
                *question_id
            }
        }
    }
}

/// The questions that stop an epic reaching `done`, in stable question order.
///
/// An undispositioned question blocks, and so does one whose deferral has been
/// reopened. Every one of the three dispositions releases its question — that
/// is what makes `deferred` an honest answer rather than a way of hiding one:
/// the deferral names the trigger that will bring the question back, and until
/// that trigger fires the question is genuinely closed.
///
/// The input is a freshly read summary set, never a snapshot taken when
/// completion started. A question raised during closeout has to count.
#[must_use]
pub fn open_question_blockers(summaries: &[OpenQuestionSummary]) -> Vec<OpenQuestionBlocker> {
    let mut blocking: Vec<&OpenQuestionSummary> = summaries
        .iter()
        .filter(|summary| summary.status.blocks_completion())
        .collect();
    blocking.sort_by_key(|summary| summary.question_id);
    blocking
        .into_iter()
        .map(|summary| match summary.status {
            OpenQuestionStatus::Reopened => OpenQuestionBlocker::Reopened {
                question_id: summary.question_id,
                subject: summary.subject.clone(),
            },
            // `blocks_completion` admits exactly `open` and `reopened`; the
            // three dispositioned states never reach here.
            _ => OpenQuestionBlocker::Undispositioned {
                question_id: summary.question_id,
                subject: summary.subject.clone(),
            },
        })
        .collect()
}
