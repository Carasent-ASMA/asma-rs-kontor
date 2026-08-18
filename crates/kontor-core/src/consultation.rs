//! Advisor profiles and Committee templates.
//!
//! A consultation is read-only advice: an Advisor answers one bounded question,
//! a Committee reaches one typed verdict, and neither of them changes a Task, a
//! phase or a gate. What they may read, who may ask them, which provider
//! answers and how their output is aggregated is *policy*, and policy that a
//! caller could restate per invocation would not be policy at all. So both
//! families are immutable versioned documents published once and pinned by
//! every run that names them.
//!
//! The two specifications below are deliberately incapable of describing a
//! mutation. There is no capability list, no operation allowlist, no scheduler
//! hook and no gate waiver anywhere in them: a profile cannot grant an
//! authority it has no field to name. The read-only boundary is enforced again
//! at the resolved runtime capability, but it starts by being unrepresentable
//! here.
//!
//! Cardinality is data. A Committee is whatever its template declares, so no
//! service may branch on "three seats" — the production preset happens to
//! freeze two reviewers and one Judge, and test-only templates exercise the
//! same path with two and five.

use crate::id::{
    AdvisorProfileId, AdvisorRunId, ArtifactKey, BoundedText, CanonicalDocument, CommitteeRunId,
    CommitteeTemplateId, ExternalName, RoleKey, RoleSlotId, SchemaVersion, SpecVersion,
};
use crate::spec::{BudgetBounds, ModelChainPolicy, ProviderRef, SkillRef};
use crate::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

/// Upper bound on the skills one consultation seat may pin.
pub const MAX_CONSULTATION_SKILLS: usize = 32;
/// Upper bound on the files one consultation seat may be granted.
pub const MAX_CONSULTATION_FILES: usize = 64;
/// Upper bound on the slots one Committee template may declare.
pub const MAX_COMMITTEE_SLOTS: usize = 16;
/// Upper bound on rounds one Committee run may spend.
///
/// Round one is the decision; round two is the single authorized re-review.
/// A third round is `remediation_budget_exhausted`, so no template may declare
/// a ceiling that would promise one.
pub const MAX_COMMITTEE_ROUNDS: u32 = 2;

/// The stable identity of either consultation family.
///
/// The public routes remain family-specific, but persistence and runtime
/// placement are deliberately shared. Carrying the discriminator with the id
/// prevents an Advisor UUID from being looked up as a Committee merely because
/// both identifiers have the same wire shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "family", content = "run_id", rename_all = "snake_case")]
pub enum ConsultationRunId {
    /// One Advisor consultation.
    Advisor(AdvisorRunId),
    /// One Committee consultation.
    Committee(CommitteeRunId),
}

impl ConsultationRunId {
    /// Which service owns this run.
    #[must_use]
    pub const fn family(self) -> ConsultationFamily {
        match self {
            Self::Advisor(_) => ConsultationFamily::Advisor,
            Self::Committee(_) => ConsultationFamily::Committee,
        }
    }

    /// The canonical UUID text shared by storage and runtime labels.
    #[must_use]
    pub fn as_text(self) -> String {
        match self {
            Self::Advisor(id) => id.to_string(),
            Self::Committee(id) => id.to_string(),
        }
    }
}

crate::closed_enum! {
    /// Which consultation family a published revision belongs to.
    ///
    /// The two families share one storage and wire shape, so the discriminator
    /// is data. It is closed rather than a free string because the route that
    /// accepts a definition chooses it — a caller never states which family its
    /// document is, and so cannot publish an Advisor profile into the Committee
    /// catalog.
    ConsultationFamily, "ConsultationFamily" {
        /// Advisor profiles.
        Advisor => "advisor",
        /// Committee templates.
        Committee => "committee",
    }
}

crate::closed_enum! {
    /// The durable lifecycle of one consultation.
    ///
    /// A run becomes `running` only after every declared native seat has been
    /// read back. Committee findings then drive the two waiting states; no
    /// runtime completion signal can skip either of them.
    ConsultationRunState, "ConsultationRunState" {
        /// The run and its frozen ids exist; native placement is incomplete.
        Materializing => "materializing",
        /// An Advisor is working, or a Committee's independent reviewers are.
        Running => "running",
        /// Every reviewer finding is durable and the Judge may now read them.
        AwaitingJudge => "awaiting_judge",
        /// The evidence-only result and disposition/verdict are immutable.
        Settled => "settled",
        /// The bounded protocol could not produce a typed result.
        NeedsHuman => "needs_human",
    }
}

crate::closed_enum! {
    /// Whether a consultation may read the project's approved memory.
    ///
    /// Memory is referenced as an access level rather than as a list of record
    /// ids because approval already lives on the memory record itself. A
    /// profile that enumerated ids would freeze a snapshot of somebody else's
    /// aggregate and drift the moment one of them was tombstoned.
    MemoryAccess, "MemoryAccess" {
        /// The frozen context pack is the whole world the seat may read.
        None => "none",
        /// Approved project memory may be resolved into the frozen pack.
        ApprovedProjectMemory => "approved_project_memory",
    }
}

crate::closed_enum! {
    /// Where a consultation may be invoked.
    ///
    /// Both values are inside one epic: a consultation with no epic has no
    /// place in the topology and nothing to advise.
    ConsultationScope, "ConsultationScope" {
        /// Asked about the epic as a whole.
        Epic => "epic",
        /// Asked about one ticket belonging to that epic.
        Ticket => "ticket",
    }
}

crate::closed_enum! {
    /// What one Committee seat is for.
    CommitteeRole, "CommitteeRole" {
        /// Produces one independent finding per round.
        Reviewer => "reviewer",
        /// Reads the durable findings and explains the recomputed outcome.
        Judge => "judge",
    }
}

crate::closed_enum! {
    /// How a Committee's findings become one outcome.
    ///
    /// Only the conjunctive rule is implemented. Jury, quorum/threshold and
    /// deliberative panel are explicit protocol deferrals, and they are absent
    /// here rather than accepted-and-ignored so a template cannot claim one and
    /// silently receive a conjunction instead.
    AggregationProtocol, "AggregationProtocol" {
        /// Every required reviewer must agree, and every required evidence set
        /// must be complete.
        Conjunctive => "conjunctive",
    }
}

crate::closed_enum! {
    /// The independence a template requires of its reviewers.
    DiversityRule, "DiversityRule" {
        /// Reviewers may share a provider. Only legitimate for fixtures that
        /// are exercising cardinality rather than independence.
        None => "none",
        /// No two reviewers may reach the same provider, on any rung of their
        /// chains. A different model or a different label on one provider is
        /// the same provider.
        DistinctProviderPerSlot => "distinct_provider_per_slot",
    }
}

crate::closed_enum! {
    /// One Committee's typed outcome.
    ///
    /// This closed pair *is* the verdict schema. A template that could declare
    /// its own vocabulary could declare one the settlement rule cannot
    /// recompute, and a verdict the server cannot recompute is a verdict the
    /// Judge decides.
    CommitteeVerdict, "CommitteeVerdict" {
        /// Every required reviewer agreed and every required evidence set was
        /// complete.
        Compliant => "compliant",
        /// Anything else, once every required finding is durable.
        NonCompliant => "non_compliant",
    }
}

crate::closed_enum! {
    /// What the requester did about one Advisor's advice.
    ///
    /// A disposition records a decision. It never rewrites the advice, grants
    /// an authority, waives a gate or asserts that a command ran — a command
    /// that ran has its own receipt, and the disposition may only cite it.
    AdviceDisposition, "AdviceDisposition" {
        /// The advice was adopted.
        Accepted => "accepted",
        /// Named parts of it were adopted.
        PartiallyAccepted => "partially_accepted",
        /// It was considered and not adopted.
        Rejected => "rejected",
        /// A later recorded decision replaces an earlier disposition.
        Superseded => "superseded",
    }
}

/// What one consultation seat is allowed to read.
///
/// Everything here is a *pin*, not a path: a revision of a skill, a key of an
/// already-authoritative artifact, an access level over already-approved
/// memory. A consultation cannot be handed a prompt, a file body or a runtime
/// state through this policy, because none of those can be named by it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsultationContextPolicy {
    /// Pinned skill revisions the seat is granted.
    pub skills: Vec<SkillRef>,
    /// Artifacts the seat may have resolved into its frozen pack.
    pub files: Vec<ArtifactKey>,
    /// Whether approved project memory may be resolved into that pack.
    pub memory: MemoryAccess,
}

impl ConsultationContextPolicy {
    /// Validate the grant.
    ///
    /// # Errors
    /// Rejects an oversized or duplicated grant. A duplicate is refused rather
    /// than deduplicated so that two revisions which differ only by a repeated
    /// entry cannot hash differently while meaning the same thing.
    pub fn validate(&self, subject: &'static str) -> DomainResult<()> {
        if self.skills.len() > MAX_CONSULTATION_SKILLS {
            return Err(DomainError::invalid(
                subject,
                "grants more skills than a consultation seat may pin",
            ));
        }
        if self.files.len() > MAX_CONSULTATION_FILES {
            return Err(DomainError::invalid(
                subject,
                "grants more files than a consultation seat may read",
            ));
        }
        if has_duplicate(&self.skills) {
            return Err(DomainError::invalid(subject, "grants one skill twice"));
        }
        if has_duplicate(&self.files) {
            return Err(DomainError::invalid(subject, "grants one file twice"));
        }
        Ok(())
    }
}

/// One immutable revision of an Advisor profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvisorProfileSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The profile id shared by every revision.
    pub profile_id: AdvisorProfileId,
    /// This revision.
    pub version: SpecVersion,
    /// Human name.
    pub name: ExternalName,
    /// The short form used in the ASW workspace title.
    pub short_name: ExternalName,
    /// The domain this Advisor is consulted about.
    pub expertise: BoundedText,
    /// The bounded behavioural prompt the seat is launched with.
    pub behavior: BoundedText,
    /// What its advice must contain to count as an answer.
    pub output_requirements: BoundedText,
    /// The ordered provider/model chain the seat may reach.
    pub models: ModelChainPolicy,
    /// What the seat may read.
    pub context: ConsultationContextPolicy,
    /// Roles allowed to consult it. Never empty.
    pub allowed_caller_roles: Vec<RoleKey>,
    /// Scopes it may be invoked at. Never empty.
    pub allowed_scopes: Vec<ConsultationScope>,
    /// Resource ceiling for one consultation.
    pub budget: BudgetBounds,
    /// How many consultations of this profile one epic may spend.
    pub max_consultations: u32,
}

impl AdvisorProfileSpec {
    /// Validate the profile.
    ///
    /// # Errors
    /// Rejects an empty or duplicated caller/scope list, an unusable model
    /// chain, a non-positive budget or consultation limit, and an oversized or
    /// duplicated context grant.
    pub fn validate(&self) -> DomainResult<()> {
        const SUBJECT: &str = "AdvisorProfileSpec";
        if self.version.get() == 0 {
            return Err(DomainError::invalid(
                SUBJECT,
                "a published revision starts at version one",
            ));
        }
        if self.expertise.as_str().trim().is_empty()
            || self.behavior.as_str().trim().is_empty()
            || self.output_requirements.as_str().trim().is_empty()
        {
            return Err(DomainError::invalid(
                SUBJECT,
                "expertise, behavior and output requirements must each say something",
            ));
        }
        self.models.validate()?;
        self.context.validate(SUBJECT)?;
        if self.allowed_caller_roles.is_empty() {
            return Err(DomainError::invalid(
                SUBJECT,
                "a profile no role may consult is unreachable",
            ));
        }
        if has_duplicate(&self.allowed_caller_roles) {
            return Err(DomainError::invalid(SUBJECT, "names one caller role twice"));
        }
        if self.allowed_scopes.is_empty() {
            return Err(DomainError::invalid(
                SUBJECT,
                "a profile with no allowed scope is unreachable",
            ));
        }
        if has_duplicate(&self.allowed_scopes) {
            return Err(DomainError::invalid(SUBJECT, "names one scope twice"));
        }
        self.budget.validate()?;
        if self.max_consultations == 0 {
            return Err(DomainError::invalid(
                SUBJECT,
                "a consultation limit of zero would publish an Advisor that cannot be asked",
            ));
        }
        Ok(())
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`AdvisorProfileSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }
}

/// One seat a Committee template declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitteeSlotSpec {
    /// The stable slot address findings are keyed by.
    pub id: RoleSlotId,
    /// What this seat is for.
    pub role: CommitteeRole,
    /// The logical role the seat is held under.
    pub logical_role: RoleKey,
    /// What this seat brings that the others do not.
    pub specialty: BoundedText,
    /// The bounded behavioural prompt the seat is launched with.
    pub behavior: BoundedText,
    /// The ordered provider/model chain the seat may reach.
    pub models: ModelChainPolicy,
    /// What the seat may read.
    pub context: ConsultationContextPolicy,
}

impl CommitteeSlotSpec {
    /// Validate the slot.
    ///
    /// # Errors
    /// Rejects empty prose, an unusable model chain and an invalid grant.
    pub fn validate(&self) -> DomainResult<()> {
        const SUBJECT: &str = "CommitteeSlotSpec";
        if self.specialty.as_str().trim().is_empty() || self.behavior.as_str().trim().is_empty() {
            return Err(DomainError::invalid(
                SUBJECT,
                "specialty and behavior must each say something",
            ));
        }
        self.models.validate()?;
        self.context.validate(SUBJECT)?;
        Ok(())
    }

    /// Every provider this slot can reach, including fallback rungs.
    ///
    /// Diversity is judged over the whole chain rather than the primary rung:
    /// a fallback that lands on the other reviewer's provider would collapse
    /// the independence the conjunction is supposed to be measuring, quietly
    /// and only under load.
    fn providers(&self) -> impl Iterator<Item = &ProviderRef> {
        self.models.rungs.iter().map(|rung| &rung.provider)
    }
}

/// One immutable revision of a Committee template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitteeTemplateSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The template id shared by every revision.
    pub template_id: CommitteeTemplateId,
    /// This revision.
    pub version: SpecVersion,
    /// Human name.
    pub name: ExternalName,
    /// The short form used in the CSW workspace title.
    pub short_name: ExternalName,
    /// The question this Committee is convened to answer.
    pub charter: BoundedText,
    /// Ordered slots. Findings are keyed by slot id, so the order is display
    /// only and the ids are the addresses.
    pub slots: Vec<CommitteeSlotSpec>,
    /// How findings become one outcome.
    pub aggregation: AggregationProtocol,
    /// The independence required of the reviewers.
    pub diversity: DiversityRule,
    /// Roles allowed to convene it. Never empty.
    pub allowed_caller_roles: Vec<RoleKey>,
    /// Scopes it may be invoked at. Never empty.
    pub allowed_scopes: Vec<ConsultationScope>,
    /// Resource ceiling for one round.
    pub budget: BudgetBounds,
    /// How many rounds one run may spend, at most [`MAX_COMMITTEE_ROUNDS`].
    pub round_limit: u32,
}

impl CommitteeTemplateSpec {
    /// Validate the template.
    ///
    /// # Errors
    /// Rejects a template that could not produce an independent conjunction:
    /// duplicate or missing slots, fewer than two reviewers, more than one
    /// Judge, reviewers sharing a provider under a distinctness rule, a round
    /// limit beyond the one authorized re-review, or an invalid slot, caller
    /// list or budget.
    pub fn validate(&self) -> DomainResult<()> {
        const SUBJECT: &str = "CommitteeTemplateSpec";
        if self.version.get() == 0 {
            return Err(DomainError::invalid(
                SUBJECT,
                "a published revision starts at version one",
            ));
        }
        if self.charter.as_str().trim().is_empty() {
            return Err(DomainError::invalid(
                SUBJECT,
                "the charter must say something",
            ));
        }
        if self.slots.len() > MAX_COMMITTEE_SLOTS {
            return Err(DomainError::invalid(
                SUBJECT,
                "declares more slots than a Committee may seat",
            ));
        }
        let ids: Vec<&RoleSlotId> = self.slots.iter().map(|slot| &slot.id).collect();
        if has_duplicate(&ids) {
            return Err(DomainError::invalid(SUBJECT, "declares one slot id twice"));
        }
        for slot in &self.slots {
            slot.validate()?;
        }

        let reviewers: Vec<&CommitteeSlotSpec> = self
            .slots
            .iter()
            .filter(|slot| slot.role == CommitteeRole::Reviewer)
            .collect();
        let judges = self
            .slots
            .iter()
            .filter(|slot| slot.role == CommitteeRole::Judge)
            .count();
        if reviewers.len() < 2 {
            return Err(DomainError::invalid(
                SUBJECT,
                "a Committee needs at least two reviewers to have anything to agree about",
            ));
        }
        if judges > 1 {
            return Err(DomainError::invalid(
                SUBJECT,
                "at most one Judge may read the findings",
            ));
        }
        if self.diversity == DiversityRule::DistinctProviderPerSlot {
            for (index, slot) in reviewers.iter().enumerate() {
                for other in &reviewers[index + 1..] {
                    if slot
                        .providers()
                        .any(|provider| other.providers().any(|candidate| candidate == provider))
                    {
                        return Err(DomainError::invalid(
                            SUBJECT,
                            "two reviewers can reach the same provider, so their findings \
                             would not be independent",
                        ));
                    }
                }
            }
        }
        if self.allowed_caller_roles.is_empty() {
            return Err(DomainError::invalid(
                SUBJECT,
                "a template no role may convene is unreachable",
            ));
        }
        if has_duplicate(&self.allowed_caller_roles) {
            return Err(DomainError::invalid(SUBJECT, "names one caller role twice"));
        }
        if self.allowed_scopes.is_empty() {
            return Err(DomainError::invalid(
                SUBJECT,
                "a template with no allowed scope is unreachable",
            ));
        }
        if has_duplicate(&self.allowed_scopes) {
            return Err(DomainError::invalid(SUBJECT, "names one scope twice"));
        }
        self.budget.validate()?;
        if self.round_limit == 0 || self.round_limit > MAX_COMMITTEE_ROUNDS {
            return Err(DomainError::invalid(
                SUBJECT,
                "a run spends one decision round and at most one authorized re-review",
            ));
        }
        Ok(())
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`CommitteeTemplateSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }

    /// The slots that must record a finding before the Judge may read them.
    #[must_use]
    pub fn reviewer_slots(&self) -> Vec<&RoleSlotId> {
        self.slots
            .iter()
            .filter(|slot| slot.role == CommitteeRole::Reviewer)
            .map(|slot| &slot.id)
            .collect()
    }

    /// The Judge slot, when the template freezes one.
    #[must_use]
    pub fn judge_slot(&self) -> Option<&RoleSlotId> {
        self.slots
            .iter()
            .find(|slot| slot.role == CommitteeRole::Judge)
            .map(|slot| &slot.id)
    }
}

/// The conjunctive rule, recomputed from durable findings.
///
/// The Judge explains this; it does not decide it. `None` means the run is
/// still awaiting a required finding and no outcome may be settled yet — a
/// missing finding is never counted as agreement, and a recorded finding with
/// incomplete evidence counts against the gate rather than being dropped from
/// the denominator.
#[must_use]
pub fn conjunctive_outcome(
    required: &[RoleSlotId],
    recorded: &[RecordedFinding],
) -> Option<CommitteeVerdict> {
    for slot in required {
        if !recorded.iter().any(|finding| &finding.slot == slot) {
            return None;
        }
    }
    let all_compliant = required.iter().all(|slot| {
        recorded
            .iter()
            .filter(|finding| &finding.slot == slot)
            .all(|finding| {
                finding.verdict == CommitteeVerdict::Compliant && finding.evidence_complete
            })
    });
    Some(if all_compliant {
        CommitteeVerdict::Compliant
    } else {
        CommitteeVerdict::NonCompliant
    })
}

/// One reviewer's durable finding, as the settlement rule reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedFinding {
    /// The frozen slot that recorded it.
    pub slot: RoleSlotId,
    /// What that reviewer concluded.
    pub verdict: CommitteeVerdict,
    /// Whether every piece of evidence the template required was cited.
    pub evidence_complete: bool,
}

/// Whether any value appears twice.
///
/// A linear scan: these lists are bounded in the tens, so a hash set would cost
/// more in `Hash` bounds on every key type than it saves in comparisons.
fn has_duplicate<T: PartialEq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[index + 1..].contains(value))
}
