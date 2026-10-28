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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::branch::{BranchName, BranchRefusal, ProjectBranchRules, TrackerKey, valid_base_branch};
use crate::id::{ContentHash, ExternalName, ModuleKey, ProjectId};
use crate::pull_request::ForgeRepository;
use crate::{DomainError, DomainResult};

/// The revision of the rules below. Bumped whenever a rule changes so a recorded
/// decision can be read against the exact policy that produced it.
pub const POLICY_REVISION: u32 = 6;

/// Project publication authority, resolved from authoring data and pinned in a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationPolicy {
    /// The existing realm project this policy may govern, never a creation request.
    pub project_id: ProjectId,
    /// The project-owned repository allowlist; membership alone grants nothing.
    pub repositories: Vec<ForgeRepository>,
    /// The repository every confirmed binding in this project may publish to.
    pub root_repository: ForgeRepository,
    /// Exact durable task module keys and their additional publication repository.
    pub module_repositories: BTreeMap<ModuleKey, ForgeRepository>,
    /// The only permitted publication base.
    pub base_branch: ExternalName,
    /// Optional selected-project grammar, pinned alongside repository authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_rules: Option<ProjectBranchRules>,
}

impl PublicationPolicy {
    /// Check that every granted repository is in the project's explicit allowlist.
    ///
    /// # Errors
    /// Refuses a root or module repository outside that list.
    pub fn validate(&self) -> DomainResult<()> {
        if !valid_base_branch(self.base_branch.as_str()) {
            return Err(DomainError::invalid(
                "PublicationPolicy",
                "invalid project.forge.base_branch",
            ));
        }
        if let Some(rules) = &self.branch_rules {
            rules
                .validate()
                .map_err(|rule| DomainError::invalid("PublicationPolicy", rule))?;
        }
        if !self.repositories.contains(&self.root_repository) {
            return Err(DomainError::invalid(
                "PublicationPolicy",
                "project.publication.root_repository must be in project.forge.repositories",
            ));
        }
        if self
            .module_repositories
            .values()
            .any(|repository| !self.repositories.contains(repository))
        {
            return Err(DomainError::invalid(
                "PublicationPolicy",
                "project.publication.module_repositories must be in project.forge.repositories",
            ));
        }
        Ok(())
    }

    /// The root plus only the exact module mappings these stored tasks contribute.
    #[must_use]
    pub fn authorized_repositories<'a>(
        &self,
        modules: impl IntoIterator<Item = &'a ModuleKey>,
    ) -> Vec<ForgeRepository> {
        let mut repositories = vec![self.root_repository.clone()];
        for module in modules {
            if let Some(repository) = self.module_repositories.get(module)
                && !repositories.contains(repository)
            {
                repositories.push(repository.clone());
            }
        }
        repositories
    }
}

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
    /// The exact forge repositories this Kontor project may publish.
    ///
    /// Derived by the daemon from the durable ProjectId the forge mapping
    /// governs and the bound task's module. A producer's observed `repository`
    /// is a claim; this is the durable fact it is judged against. An empty set
    /// authorizes nothing, which is the fail-closed answer for a project the
    /// mapping does not govern.
    pub repositories: Vec<ExternalName>,
    /// The confirmed tracker key of the epic.
    pub epic_key: TrackerKey,
    /// The confirmed tracker key of the task the branch names, when the branch
    /// names a task rather than the epic.
    pub task_key: Option<TrackerKey>,
    /// Every confirmed task key in the epic. Retained in the frozen binding as
    /// evidence of the resolved graph; it never widens the branch identity.
    pub child_keys: Vec<TrackerKey>,
    /// The project's configured publication base, the only permitted base.
    pub default_branch: ExternalName,
}

impl PublicationBinding {
    /// Every key that may appear in a title under this branch binding.
    ///
    /// The one key a title may carry under this branch binding.
    ///
    /// Branch and title always name the same confirmed Jira object. Work for a
    /// child task uses that child's branch rather than borrowing the epic
    /// checkout, which makes the rule independently enforceable at the forge.
    fn title_keys(&self) -> Box<dyn Iterator<Item = &TrackerKey> + '_> {
        match self.task_key.as_ref() {
            Some(task_key) => Box::new(std::iter::once(task_key)),
            None => Box::new(std::iter::once(&self.epic_key)),
        }
    }
}

crate::closed_enum! {
    /// Why a publication was refused. Each spelling is the stable code a producer
    /// reports; the branch codes are [`BranchRefusal`]'s own spellings.
    PublicationRefusal, "PublicationRefusal" {
        /// No activated bundle declares project publication authority.
        PolicyUnconfigured => "publication_policy_unconfigured",
        /// The activated policy or its immutable artifacts cannot be verified.
        PolicyInvalid => "publication_policy_invalid",
        /// The activated policy governs another internal project identity.
        ProjectMismatch => "publication_project_mismatch",
        /// A selected integration is bound to another internal project identity.
        ConsumerMismatch => "publication_consumer_mismatch",
        /// The forge repository is outside the project's approved publication scope.
        RepositoryMismatch => "repository_mismatch",
        /// The head branch is not the branch the binding allows.
        BranchBindingMismatch => "branch_binding_mismatch",
        /// No confirmed tracker key exists for the branch's key.
        BindingUnconfirmed => "binding_unconfirmed",
        /// More than one epic or task resolves the branch's key.
        BindingAmbiguous => "binding_ambiguous",
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
            Self::PolicyUnconfigured => {
                "publication_policy_unconfigured: activate an explicit project publication policy"
            }
            Self::PolicyInvalid => {
                "publication_policy_invalid: the activated publication policy must verify"
            }
            Self::ProjectMismatch => {
                "publication_project_mismatch: the requested project must equal the activated policy project"
            }
            Self::ConsumerMismatch => {
                "publication_consumer_mismatch: integration project bindings must agree with the activated policy"
            }
            Self::RepositoryMismatch => {
                "repository_mismatch: the publication repository must be in the project's approved scope"
            }
            Self::BranchBindingMismatch => BranchRefusal::BindingMismatch.rule(),
            Self::BindingUnconfirmed => BranchRefusal::BindingUnconfirmed.rule(),
            Self::BindingAmbiguous => {
                "binding_ambiguous: the branch key must resolve to exactly one confirmed epic or task"
            }
            Self::TitleKeyMissing => {
                "pr_title_key_missing: a pull-request title starts with the canonical tracker key and one space"
            }
            Self::TitleKeyMismatch => {
                "pr_title_key_mismatch: the pull-request title must carry the same confirmed Jira key as its branch"
            }
            Self::BaseBranchNotDefault => {
                "base_branch_not_default: a publication targets the configured project base branch"
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
    /// The immutable activated policy bundle read for this judgement, when configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_bundle_hash: Option<ContentHash>,
}

impl PublicationDecision {
    /// An acceptance under the current rules.
    #[must_use]
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            reasons: Vec::new(),
            policy_revision: POLICY_REVISION,
            source_bundle_hash: None,
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
            source_bundle_hash: None,
        }
    }

    /// The decision for a publication whose branch key Kontor holds no binding for.
    #[must_use]
    pub fn unconfirmed() -> Self {
        Self::refused_branch(BranchRefusal::BindingUnconfirmed)
    }

    /// The decision for a branch key that resolves to more than one subject.
    #[must_use]
    pub fn ambiguous() -> Self {
        Self {
            accepted: false,
            reasons: vec![PublicationRefusal::BindingAmbiguous.as_str().to_owned()],
            policy_revision: POLICY_REVISION,
            source_bundle_hash: None,
        }
    }

    /// Refuse an unavailable or mismatched publication authority without resolving a binding.
    #[must_use]
    pub fn refused(refusal: PublicationRefusal) -> Self {
        Self {
            accepted: false,
            reasons: vec![refusal.as_str().to_owned()],
            policy_revision: POLICY_REVISION,
            source_bundle_hash: None,
        }
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
    if !binding.repositories.contains(&identity.repository) {
        reasons.push(PublicationRefusal::RepositoryMismatch);
    }
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
    match (identity.pull_request, identity.title.as_ref()) {
        // A pull request always carries a title, so an absent one is the
        // missing key rather than a rule that does not apply. Skipping it let a
        // pull request publish under no key at all.
        (Some(_), None) => reasons.push(PublicationRefusal::TitleKeyMissing),
        (_, Some(title)) => match title_key(title.as_str()) {
            None => reasons.push(PublicationRefusal::TitleKeyMissing),
            Some(key) if !binding.title_keys().any(|allowed| *allowed == key) => {
                reasons.push(PublicationRefusal::TitleKeyMismatch);
            }
            Some(_) => {}
        },
        // Branch-only publication: a push carries no title to judge.
        (None, None) => {}
    }
    PublicationDecision {
        accepted: reasons.is_empty(),
        reasons: reasons
            .iter()
            .map(|refusal: &PublicationRefusal| refusal.as_str().to_owned())
            .collect(),
        policy_revision: POLICY_REVISION,
        source_bundle_hash: None,
    }
}
