//! Publication identity: whether one branch, commit and pull request may be
//! published under the confirmed tracker binding Kontor holds (ASMA-8101).
//!
//! The 2026-09-05 audit found that a plausible-looking key in a branch name was
//! the only thing anyone checked. This module is the one contract behind the
//! ASMA CLI's push and pull-request steps and behind the forge check: it takes
//! the facts a producer *observed* ([`PublicationIdentity`]) and the facts Kontor
//! *holds* ([`PublicationBinding`]) and answers with a typed decision whose
//! reasons are stable codes. It never talks to a repository, a tracker or a
//! forge; the daemon assembles the binding from durable state.

use serde::{Deserialize, Serialize};

use crate::branch::{BranchName, BranchRefusal, TrackerKey};
use crate::id::ExternalName;
use crate::{DomainError, DomainResult};

/// The revision of the rules below. Bumped whenever a rule changes so a recorded
/// decision can be read against the exact policy that produced it.
pub const POLICY_REVISION: u32 = 2;

/// A Git commit identity: exactly forty lowercase hexadecimal digits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommitSha(String);

impl CommitSha {
    /// Parse a full SHA-1 object name.
    ///
    /// # Errors
    /// Refuses abbreviated, uppercase or non-hexadecimal text: an abbreviation
    /// is not an identity and a case-normalised copy is not the observed one.
    pub fn parse(text: &str) -> DomainResult<Self> {
        if text.len() != 40
            || !text
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(DomainError::invalid(
                "CommitSha",
                "must be exactly forty lowercase hexadecimal digits",
            ));
        }
        Ok(Self(text.to_owned()))
    }

    /// The exact text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a producer observed about the publication it is about to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationIdentity {
    /// The forge repository, as `owner/name`.
    pub repository: ExternalName,
    /// The branch the publication targets.
    pub base_branch: ExternalName,
    /// The branch being published. Already canonical: a name outside the
    /// grammar cannot become an identity at all.
    pub head_branch: BranchName,
    /// The exact commit at the head of that branch.
    pub head_sha: CommitSha,
    /// The pull request carrying the branch, when one exists.
    pub pull_request: Option<u64>,
    /// The pull-request title, when one exists or is about to be created.
    pub title: Option<ExternalName>,
}

/// What Kontor holds about the work a publication claims to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationBinding {
    /// The confirmed tracker key of the epic.
    pub epic_key: TrackerKey,
    /// The confirmed tracker key of the task the branch names, when the branch
    /// names a task rather than the epic.
    pub task_key: Option<TrackerKey>,
    /// Every confirmed task key in the epic. A pull-request title may name any
    /// of them: the branch represents the epic and the title names a child.
    pub child_keys: Vec<TrackerKey>,
    /// The repository's default branch, the only permitted base.
    pub default_branch: ExternalName,
}

impl PublicationBinding {
    /// Every key that may appear in a title under this branch binding.
    ///
    /// An epic branch may publish one of its child tasks. A task branch is
    /// narrower: its title must carry that same task key, so a sibling cannot
    /// silently reuse the checkout and publication identity.
    fn title_keys(&self) -> Box<dyn Iterator<Item = &TrackerKey> + '_> {
        match self.task_key.as_ref() {
            Some(task_key) => Box::new(std::iter::once(task_key)),
            None => Box::new(std::iter::once(&self.epic_key).chain(self.child_keys.iter())),
        }
    }
}

crate::closed_enum! {
    /// Why a publication was refused. Each spelling is the stable code a producer
    /// reports; the branch codes are [`BranchRefusal`]'s own spellings.
    PublicationRefusal, "PublicationRefusal" {
        /// The head branch is not the branch the binding allows.
        BranchBindingMismatch => "branch_binding_mismatch",
        /// No confirmed tracker key exists for the branch's key.
        BindingUnconfirmed => "binding_unconfirmed",
        /// The title does not lead with a canonical key and one space.
        TitleKeyMissing => "pr_title_key_missing",
        /// The title leads with a key outside the epic graph.
        TitleKeyMismatch => "pr_title_key_mismatch",
        /// The publication does not target the default branch.
        BaseBranchNotDefault => "base_branch_not_default",
    }
}

impl PublicationRefusal {
    /// The rule that was violated, prefixed with the stable code.
    #[must_use]
    pub const fn rule(self) -> &'static str {
        match self {
            Self::BranchBindingMismatch => BranchRefusal::BindingMismatch.rule(),
            Self::BindingUnconfirmed => BranchRefusal::BindingUnconfirmed.rule(),
            Self::TitleKeyMissing => {
                "pr_title_key_missing: a pull-request title starts with the canonical tracker key and one space"
            }
            Self::TitleKeyMismatch => {
                "pr_title_key_mismatch: the pull-request title names a key outside this epic's graph"
            }
            Self::BaseBranchNotDefault => {
                "base_branch_not_default: a publication targets the repository's default branch"
            }
        }
    }
}

/// The typed answer to one publication question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationDecision {
    /// Whether every rule held.
    pub accepted: bool,
    /// Every rule that failed, in evaluation order, as the stable spellings of
    /// [`BranchRefusal`] and [`PublicationRefusal`].
    pub reasons: Vec<String>,
    /// The rules revision this decision was computed under.
    pub policy_revision: u32,
}

impl PublicationDecision {
    /// An acceptance under the current rules.
    #[must_use]
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            reasons: Vec::new(),
            policy_revision: POLICY_REVISION,
        }
    }

    /// The decision for a head branch outside the grammar: nothing else can be
    /// judged because there is no identity to judge.
    #[must_use]
    pub fn refused_branch(refusal: BranchRefusal) -> Self {
        Self {
            accepted: false,
            reasons: vec![refusal.as_str().to_owned()],
            policy_revision: POLICY_REVISION,
        }
    }

    /// The decision for a publication whose branch key Kontor holds no binding for.
    #[must_use]
    pub fn unconfirmed() -> Self {
        Self::refused_branch(BranchRefusal::BindingUnconfirmed)
    }

    /// Whether a given stable code is among the reasons.
    #[must_use]
    pub fn refused_for(&self, code: &str) -> bool {
        self.reasons.iter().any(|reason| reason == code)
    }
}

/// The key a title leads with, when it leads with a canonical key and one space.
#[must_use]
pub fn title_key(title: &str) -> Option<TrackerKey> {
    let (head, rest) = title.split_once(' ')?;
    if rest.trim().is_empty() {
        return None;
    }
    TrackerKey::parse(head).ok()
}

/// Judge one publication against the binding Kontor holds.
///
/// Every rule is evaluated so the producer learns everything wrong at once; a
/// refusal is a decision with reasons, never an error.
#[must_use]
pub fn evaluate(
    identity: &PublicationIdentity,
    binding: &PublicationBinding,
) -> PublicationDecision {
    let mut reasons = Vec::new();
    let allowed_branch_keys = std::iter::once(&binding.epic_key).chain(binding.task_key.iter());
    if identity
        .head_branch
        .ensure_bound_to(allowed_branch_keys)
        .is_err()
    {
        reasons.push(PublicationRefusal::BranchBindingMismatch);
    }
    if identity.base_branch != binding.default_branch {
        reasons.push(PublicationRefusal::BaseBranchNotDefault);
    }
    if let Some(title) = identity.title.as_ref() {
        match title_key(title.as_str()) {
            None => reasons.push(PublicationRefusal::TitleKeyMissing),
            Some(key) if !binding.title_keys().any(|allowed| *allowed == key) => {
                reasons.push(PublicationRefusal::TitleKeyMismatch);
            }
            Some(_) => {}
        }
    }
    PublicationDecision {
        accepted: reasons.is_empty(),
        reasons: reasons
            .iter()
            .map(|refusal: &PublicationRefusal| refusal.as_str().to_owned())
            .collect(),
        policy_revision: POLICY_REVISION,
    }
}
