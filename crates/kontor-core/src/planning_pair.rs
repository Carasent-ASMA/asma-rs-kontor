//! The planning pair: two independent read-only members, one findings round,
//! at most one clarification round the caller asks for, and the caller's own
//! decision (ASMA-8282).
//!
//! `planning_pair@1` is advice. It is not a Committee with its Judge removed:
//! it has no Judge, no quorum, no conjunctive verdict and no settlement, and
//! nothing it records has the shape a formal review gate reads. Its bounds are
//! the protocol version's, not data a document could vary — exactly two member
//! slots, `SEAT A` and `SEAT B`, one independent findings round and at most one
//! clarification round — so a successor that changes a bound is a new protocol
//! version the caller selects explicitly, never an edited document.
//!
//! The two members are frozen once, from one placement the shared fleet
//! allocator made on the activated snapshot under distinct actual vendors
//! (`kontor-fleet-activation`), and never change afterwards. Each member
//! records its finding without seeing the other's: neither finding is released
//! to anybody until both are durable. The caller may then ask one
//! clarification question of one or both members, and finally records what it
//! decided about each member's advice. That disposition must cite every
//! member's exact finding and answer, so a member the caller disagreed with
//! keeps its dissent on the record rather than being dropped from it.
//!
//! Selection among an Advisor, a planning pair and a formal Independent Review
//! is the caller's: [`select_protocol`] returns the protocol the caller named
//! or refuses, and never substitutes another. Only
//! [`ConsultationProtocol::IndependentReview`] passes
//! [`ConsultationProtocol::require_formal_review`].

use crate::consultation::{
    AdviceDisposition, ConsultationContextPolicy, ConsultationScope, has_duplicate,
};
use crate::id::{
    BoundedText, CanonicalDocument, ContentHash, ExternalName, RoleKey, SchemaVersion,
};
use crate::spec::{BudgetBounds, ModelRung};
use crate::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

/// The protocol's name and version, as a caller selects it.
pub const PLANNING_PAIR_PROTOCOL: &str = "planning_pair@1";

/// Findings rounds one planning pair spends: exactly one.
pub const FINDINGS_ROUNDS: u32 = 1;

/// Clarification rounds one planning pair may spend: at most one, and only
/// when the caller asks for it.
pub const MAX_CLARIFICATION_ROUNDS: u32 = 1;

/// The stable rule texts a refusal names.
#[allow(
    missing_docs,
    reason = "each constant is documented by the verbatim text it holds"
)]
pub mod rule {
    pub const NO_PROTOCOL: &str =
        "a consultation names its protocol; none is chosen for the caller";
    pub const UNAVAILABLE: &str =
        "the selected protocol is not available here, and no other protocol is substituted for it";
    pub const NOT_FORMAL: &str = "advice from an Advisor or a planning pair cannot satisfy a formal review gate; only a formal Independent Review can";
    pub const NOT_PLANNING_PAIR: &str = "a planning pair document names planning_pair@1";
    pub const MEMBERS: &str = "a planning pair has exactly two members, seat-a then seat-b";
    pub const VENDOR_UNKNOWN: &str = "a planning pair member needs a known actual vendor";
    pub const SAME_VENDOR: &str =
        "the two members reach the same actual vendor, so their findings would not be independent";
    pub const MEMBER_ONLY: &str =
        "only the member frozen in a slot records that slot's finding or answer";
    pub const CALLER_ONLY: &str =
        "only the caller asks for clarification or records the disposition";
    pub const FINDING_IMMUTABLE: &str = "a recorded finding is immutable";
    pub const ANSWER_IMMUTABLE: &str = "a recorded clarification answer is immutable";
    pub const FINDINGS_INCOMPLETE: &str =
        "both members' findings must be recorded before the caller acts on either";
    pub const EXTRA_CLARIFICATION: &str = "a planning pair spends at most one clarification round";
    pub const NO_CLARIFICATION: &str = "no clarification was requested";
    pub const ADDRESSEES: &str =
        "a clarification question addresses one or both members, each once";
    pub const NOT_ADDRESSED: &str = "the clarification question was not addressed to this member";
    pub const ANSWERS_INCOMPLETE: &str =
        "every addressed member must answer before the caller decides";
    pub const DISSENT_LOST: &str = "a disposition addresses seat-a then seat-b, each with its exact finding and answer; omitting or rewriting one would lose its dissent";
    pub const FIRST_DISPOSITION: &str =
        "a planning pair's disposition is its only decision, so it supersedes nothing";
    pub const SAY_SOMETHING: &str = "the text must say something";
}

use rule::{
    ADDRESSEES, ANSWER_IMMUTABLE, ANSWERS_INCOMPLETE, CALLER_ONLY, DISSENT_LOST,
    EXTRA_CLARIFICATION, FINDING_IMMUTABLE, FINDINGS_INCOMPLETE, FIRST_DISPOSITION, MEMBER_ONLY,
    MEMBERS, NO_CLARIFICATION, NO_PROTOCOL, NOT_ADDRESSED, NOT_FORMAL, NOT_PLANNING_PAIR,
    SAME_VENDOR, SAY_SOMETHING, UNAVAILABLE, VENDOR_UNKNOWN,
};

crate::closed_enum! {
    /// Which consultation protocol a caller selected.
    ///
    /// The three are different protocols, not strengths of one: an Advisor
    /// answers alone, a planning pair gives two independent findings the
    /// caller decides between, and a formal Independent Review reaches one
    /// conjunctive verdict its Judge explains. None stands in for another.
    ConsultationProtocol, "ConsultationProtocol" {
        /// One Advisor profile.
        Advisor => "advisor",
        /// The bounded two-member planning pair.
        PlanningPair => "planning_pair@1",
        /// The formal conjunctive Committee protocol.
        IndependentReview => "independent_review",
    }
}

impl ConsultationProtocol {
    /// Whether a consultation of this protocol can be the evidence a formal
    /// review gate reads.
    #[must_use]
    pub const fn is_formal_review(self) -> bool {
        matches!(self, Self::IndependentReview)
    }

    /// Admit this protocol where a formal review gate reads its evidence.
    ///
    /// # Errors
    /// Refuses an Advisor and a planning pair: advice never satisfies a gate,
    /// however many members gave it or however unanimous they were.
    pub fn require_formal_review(self) -> DomainResult<()> {
        if self.is_formal_review() {
            Ok(())
        } else {
            Err(DomainError::MissingAuthority {
                subject: "FormalReviewGate",
                rule: NOT_FORMAL,
            })
        }
    }
}

/// Admit the caller's explicit protocol choice against the protocols
/// available where it asks.
///
/// The answer is the protocol the caller named, or a refusal. Nothing here
/// picks a protocol for a caller that named none, and an unavailable protocol
/// is never answered with a neighbouring one — a planning pair does not stand
/// in for an Independent Review, and an Advisor does not stand in for either.
///
/// # Errors
/// Refuses a request that names no protocol, and one whose protocol is not
/// available.
pub fn select_protocol(
    requested: Option<ConsultationProtocol>,
    available: &[ConsultationProtocol],
) -> DomainResult<ConsultationProtocol> {
    let requested =
        requested.ok_or_else(|| DomainError::invalid("ConsultationProtocol", NO_PROTOCOL))?;
    if available.contains(&requested) {
        Ok(requested)
    } else {
        Err(DomainError::invalid("ConsultationProtocol", UNAVAILABLE))
    }
}

crate::closed_enum! {
    /// One of the planning pair's two member slots.
    PlanningPairSlot, "PlanningPairSlot" {
        /// The first member, titled `SEAT A`.
        SeatA => "seat-a",
        /// The second member, titled `SEAT B`.
        SeatB => "seat-b",
    }
}

impl PlanningPairSlot {
    /// The exact native seat title.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SeatA => "SEAT A",
            Self::SeatB => "SEAT B",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::SeatA => 0,
            Self::SeatB => 1,
        }
    }
}

crate::closed_enum! {
    /// Which round a member's contribution belongs to.
    PlanningPairRound, "PlanningPairRound" {
        /// The one independent findings round.
        Findings => "findings",
        /// The optional clarification round the caller asked for.
        Clarification => "clarification",
    }
}

/// One member slot a planning pair declares.
///
/// Like every consultation seat it can name only pins — skill revisions,
/// artifact keys and an access level over approved memory — so a member has
/// no field through which a write, a command or a gate waiver could be
/// granted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningPairMemberSpec {
    /// Which of the two slots this is.
    pub slot: PlanningPairSlot,
    /// What this member brings that the other does not.
    pub specialty: BoundedText,
    /// The bounded behavioural prompt the member is launched with.
    pub behavior: BoundedText,
    /// What the member may read.
    pub context: ConsultationContextPolicy,
}

/// One planning pair document.
///
/// Its fields are the whole protocol: there is no Judge slot, no aggregation
/// rule, no diversity switch and no round limit to set, because
/// `planning_pair@1` fixes all four.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningPairSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// Always [`ConsultationProtocol::PlanningPair`].
    pub protocol: ConsultationProtocol,
    /// Human name.
    pub name: ExternalName,
    /// The planning question this pair is convened for.
    pub charter: BoundedText,
    /// Exactly two members, `seat-a` then `seat-b`.
    pub members: Vec<PlanningPairMemberSpec>,
    /// Roles allowed to convene it. Never empty.
    pub allowed_caller_roles: Vec<RoleKey>,
    /// Scopes it may be convened at. Never empty.
    pub allowed_scopes: Vec<ConsultationScope>,
    /// Resource ceiling for one consultation.
    pub budget: BudgetBounds,
}

impl PlanningPairSpec {
    /// Validate the document.
    ///
    /// # Errors
    /// Refuses another protocol, anything but the two members in slot order,
    /// empty prose, an invalid grant, an empty or duplicated caller or scope
    /// list, and an invalid budget.
    pub fn validate(&self) -> DomainResult<()> {
        const SUBJECT: &str = "PlanningPairSpec";
        if self.protocol != ConsultationProtocol::PlanningPair {
            return Err(DomainError::invalid(SUBJECT, NOT_PLANNING_PAIR));
        }
        if self.charter.as_str().trim().is_empty() {
            return Err(DomainError::invalid(SUBJECT, SAY_SOMETHING));
        }
        let slots: Vec<PlanningPairSlot> = self.members.iter().map(|member| member.slot).collect();
        if slots != PlanningPairSlot::ALL {
            return Err(DomainError::invalid(SUBJECT, MEMBERS));
        }
        for member in &self.members {
            if member.specialty.as_str().trim().is_empty()
                || member.behavior.as_str().trim().is_empty()
            {
                return Err(DomainError::invalid(SUBJECT, SAY_SOMETHING));
            }
            member.context.validate(SUBJECT)?;
        }
        if self.allowed_caller_roles.is_empty() {
            return Err(DomainError::invalid(
                SUBJECT,
                "a planning pair no role may convene is unreachable",
            ));
        }
        if has_duplicate(&self.allowed_caller_roles) {
            return Err(DomainError::invalid(SUBJECT, "names one caller role twice"));
        }
        if self.allowed_scopes.is_empty() {
            return Err(DomainError::invalid(
                SUBJECT,
                "a planning pair with no allowed scope is unreachable",
            ));
        }
        if has_duplicate(&self.allowed_scopes) {
            return Err(DomainError::invalid(SUBJECT, "names one scope twice"));
        }
        self.budget.validate()
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`PlanningPairSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }
}

/// One frozen member: its slot, the caller-named binding it was placed
/// through, and the route and actual vendor the shared allocator chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanningPairMember {
    /// Which of the two slots it fills.
    pub slot: PlanningPairSlot,
    /// The existing binding key the caller named for this member.
    pub binding_key: String,
    /// The exact account alias, model and effort.
    pub route: ModelRung,
    /// The model's maker, as the activated policy names it. This, not the
    /// account alias or the harness, is what the two members must not share.
    pub vendor: String,
}

/// The two frozen members of one planning pair, and the placement receipt
/// they were frozen from.
///
/// There is no way to change a member once frozen: no setter, no replacement
/// and no third slot. A different pair is a different placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanningPairMembers {
    placement_hash: ContentHash,
    members: [PlanningPairMember; 2],
}

impl PlanningPairMembers {
    /// Freeze exactly two members from one placement.
    ///
    /// # Errors
    /// Refuses anything but `seat-a` then `seat-b`, an invalid route, a
    /// member whose vendor is empty or `unknown` (the fleet policy's spelling
    /// for a model whose maker it does not know), and two members on the same
    /// actual vendor.
    pub fn freeze(
        placement_hash: ContentHash,
        members: Vec<PlanningPairMember>,
    ) -> DomainResult<Self> {
        const SUBJECT: &str = "PlanningPairMembers";
        let members: [PlanningPairMember; 2] = members
            .try_into()
            .map_err(|_| DomainError::invalid(SUBJECT, MEMBERS))?;
        if members[0].slot != PlanningPairSlot::SeatA || members[1].slot != PlanningPairSlot::SeatB
        {
            return Err(DomainError::invalid(SUBJECT, MEMBERS));
        }
        for member in &members {
            if member.binding_key.trim().is_empty() {
                return Err(DomainError::invalid(
                    SUBJECT,
                    "a member names the binding it was placed through",
                ));
            }
            member.route.validate()?;
            if member.vendor.trim().is_empty() || member.vendor == "unknown" {
                return Err(DomainError::invalid(SUBJECT, VENDOR_UNKNOWN));
            }
        }
        if members[0].vendor == members[1].vendor {
            return Err(DomainError::invalid(SUBJECT, SAME_VENDOR));
        }
        Ok(Self {
            placement_hash,
            members,
        })
    }

    /// The canonical hash of the placement receipt these members came from.
    #[must_use]
    pub const fn placement_hash(&self) -> &ContentHash {
        &self.placement_hash
    }

    /// Both members, `seat-a` then `seat-b`.
    #[must_use]
    pub const fn members(&self) -> &[PlanningPairMember; 2] {
        &self.members
    }

    /// The member frozen in `slot`.
    #[must_use]
    pub const fn member(&self, slot: PlanningPairSlot) -> &PlanningPairMember {
        &self.members[slot.index()]
    }
}

/// Who acts on a planning pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningPairActor {
    /// The caller that convened the pair and owns its question and decision.
    Caller,
    /// The member frozen in one slot.
    Member(PlanningPairSlot),
}

/// One member's contribution to one round, and the hash a disposition cites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanningPairContribution {
    /// The round it belongs to.
    pub round: PlanningPairRound,
    /// The member that gave it.
    pub slot: PlanningPairSlot,
    /// The advice, verbatim.
    pub advice: BoundedText,
    /// Canonical hash of the contribution, bound to this pair's document and
    /// placement.
    pub document_hash: ContentHash,
}

/// The caller's one clarification question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationRequest {
    /// What the caller needs clarified.
    pub question: BoundedText,
    /// The members asked: one or both, each once.
    pub addressed: Vec<PlanningPairSlot>,
}

/// What the caller decided about one member's advice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberDisposition {
    /// The member.
    pub slot: PlanningPairSlot,
    /// The exact finding the decision is about.
    pub finding: ContentHash,
    /// The exact clarification answer, when the member gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<ContentHash>,
    /// Accepted, partially accepted or rejected.
    pub disposition: AdviceDisposition,
}

/// The caller's decision about the pair's advice.
///
/// It records a decision. It is not a verdict, has no field for one, and
/// nothing reads it as a gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningPairDisposition {
    /// One entry per member, `seat-a` then `seat-b`.
    pub members: Vec<MemberDisposition>,
    /// Why the caller decided as it did.
    pub rationale: BoundedText,
}

crate::closed_enum! {
    /// Where one planning pair stands.
    ///
    /// There is no settled state: the pair ends when the caller records its
    /// disposition, and nothing is computed from the findings.
    PlanningPairState, "PlanningPairState" {
        /// At least one member's finding is not yet recorded; none is released.
        AwaitingFindings => "awaiting_findings",
        /// Both findings are recorded and released to the caller.
        FindingsReleased => "findings_released",
        /// The caller's clarification question awaits an addressed member.
        AwaitingAnswers => "awaiting_answers",
        /// Every addressed member answered; the answers are released.
        AnswersReleased => "answers_released",
        /// The caller recorded its disposition. Nothing changes afterwards.
        Disposed => "disposed",
    }
}

/// The caller's clarification round: its question and each addressed
/// member's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanningPairClarification {
    request: ClarificationRequest,
    answers: [Option<PlanningPairContribution>; 2],
}

/// One planning pair consultation.
///
/// Deliberately not serializable: the only ways to read a finding are
/// [`Self::findings`] and [`Self::retained_dissent`], so no rendering of the
/// run can release one member's finding before the other's is durable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningPairRun {
    spec_hash: ContentHash,
    members: PlanningPairMembers,
    question: BoundedText,
    findings: [Option<PlanningPairContribution>; 2],
    clarification: Option<PlanningPairClarification>,
    disposition: Option<PlanningPairDisposition>,
}

impl PlanningPairRun {
    /// Convene one pair: its document, its two frozen members and the
    /// caller's question.
    ///
    /// # Errors
    /// As [`PlanningPairSpec::canonicalize`], and an empty question.
    pub fn admit(
        spec: &PlanningPairSpec,
        members: PlanningPairMembers,
        question: BoundedText,
    ) -> DomainResult<Self> {
        let spec_hash = spec.canonicalize()?.hash().clone();
        if question.as_str().trim().is_empty() {
            return Err(DomainError::invalid("PlanningPairRun", SAY_SOMETHING));
        }
        Ok(Self {
            spec_hash,
            members,
            question,
            findings: [None, None],
            clarification: None,
            disposition: None,
        })
    }

    /// Always [`ConsultationProtocol::PlanningPair`].
    #[must_use]
    pub const fn protocol(&self) -> ConsultationProtocol {
        ConsultationProtocol::PlanningPair
    }

    /// The canonical hash of the document the pair was convened under.
    #[must_use]
    pub const fn spec_hash(&self) -> &ContentHash {
        &self.spec_hash
    }

    /// The two frozen members.
    #[must_use]
    pub const fn members(&self) -> &PlanningPairMembers {
        &self.members
    }

    /// The caller's question.
    #[must_use]
    pub const fn question(&self) -> &BoundedText {
        &self.question
    }

    /// Where the pair stands.
    #[must_use]
    pub fn state(&self) -> PlanningPairState {
        if self.disposition.is_some() {
            PlanningPairState::Disposed
        } else if self.findings.iter().any(Option::is_none) {
            PlanningPairState::AwaitingFindings
        } else {
            match &self.clarification {
                None => PlanningPairState::FindingsReleased,
                Some(clarification) if clarification.is_answered() => {
                    PlanningPairState::AnswersReleased
                }
                Some(_) => PlanningPairState::AwaitingAnswers,
            }
        }
    }

    /// Both findings, once both are recorded; nothing before.
    ///
    /// The findings round is independent because neither finding is readable
    /// by the caller or by the other member until both are durable.
    #[must_use]
    pub fn findings(&self) -> Option<[&PlanningPairContribution; 2]> {
        match &self.findings {
            [Some(a), Some(b)] => Some([a, b]),
            _ => None,
        }
    }

    /// The clarification question and its answers, once every addressed
    /// member has answered; nothing before.
    #[must_use]
    pub fn clarification(&self) -> Option<(&ClarificationRequest, Vec<&PlanningPairContribution>)> {
        let clarification = self.clarification.as_ref()?;
        if !clarification.is_answered() {
            return None;
        }
        Some((
            &clarification.request,
            clarification.answers.iter().flatten().collect(),
        ))
    }

    /// The caller's disposition, once recorded.
    #[must_use]
    pub const fn disposition(&self) -> Option<&PlanningPairDisposition> {
        self.disposition.as_ref()
    }

    /// The advice the caller did not wholly adopt: every finding and answer
    /// of a member it rejected or accepted only in part, kept verbatim beside
    /// the decision. Empty until the disposition is recorded.
    #[must_use]
    pub fn retained_dissent(&self) -> Vec<&PlanningPairContribution> {
        let Some(disposition) = &self.disposition else {
            return Vec::new();
        };
        disposition
            .members
            .iter()
            .filter(|member| member.disposition != AdviceDisposition::Accepted)
            .flat_map(|member| {
                let index = member.slot.index();
                let answer = self
                    .clarification
                    .as_ref()
                    .and_then(|clarification| clarification.answers[index].as_ref());
                self.findings[index].iter().chain(answer)
            })
            .collect()
    }

    /// Record one member's finding in the findings round.
    ///
    /// # Errors
    /// [`DomainError::Terminal`] after the disposition; a missing authority
    /// unless the actor is the member frozen in `slot`; and a refusal to
    /// record a slot's finding a second time.
    pub fn record_finding(
        &mut self,
        actor: PlanningPairActor,
        slot: PlanningPairSlot,
        advice: BoundedText,
    ) -> DomainResult<ContentHash> {
        self.open()?;
        Self::member_only(actor, slot)?;
        if self.findings[slot.index()].is_some() {
            return Err(DomainError::invalid("PlanningPairRun", FINDING_IMMUTABLE));
        }
        let contribution = self.contribution(PlanningPairRound::Findings, slot, advice)?;
        let hash = contribution.document_hash.clone();
        self.findings[slot.index()] = Some(contribution);
        Ok(hash)
    }

    /// Ask the one clarification question.
    ///
    /// # Errors
    /// [`DomainError::Terminal`] after the disposition; a missing authority
    /// unless the actor is the caller; missing evidence until both findings
    /// are recorded; a second clarification round; and a question that is
    /// empty or does not address one or both members, each once.
    pub fn request_clarification(
        &mut self,
        actor: PlanningPairActor,
        request: ClarificationRequest,
    ) -> DomainResult<()> {
        self.open()?;
        Self::caller_only(actor)?;
        self.findings_complete()?;
        if self.clarification.is_some() {
            return Err(DomainError::invalid("PlanningPairRun", EXTRA_CLARIFICATION));
        }
        if request.addressed.is_empty() || has_duplicate(&request.addressed) {
            return Err(DomainError::invalid("PlanningPairRun", ADDRESSEES));
        }
        if request.question.as_str().trim().is_empty() {
            return Err(DomainError::invalid("PlanningPairRun", SAY_SOMETHING));
        }
        self.clarification = Some(PlanningPairClarification {
            request,
            answers: [None, None],
        });
        Ok(())
    }

    /// Record one addressed member's answer to the clarification question.
    ///
    /// # Errors
    /// [`DomainError::Terminal`] after the disposition; a missing authority
    /// unless the actor is the member frozen in `slot`; no clarification
    /// requested; a member the question did not address; and a second answer.
    pub fn record_answer(
        &mut self,
        actor: PlanningPairActor,
        slot: PlanningPairSlot,
        advice: BoundedText,
    ) -> DomainResult<ContentHash> {
        self.open()?;
        Self::member_only(actor, slot)?;
        let Some(clarification) = &self.clarification else {
            return Err(DomainError::invalid("PlanningPairRun", NO_CLARIFICATION));
        };
        if !clarification.request.addressed.contains(&slot) {
            return Err(DomainError::invalid("PlanningPairRun", NOT_ADDRESSED));
        }
        if clarification.answers[slot.index()].is_some() {
            return Err(DomainError::invalid("PlanningPairRun", ANSWER_IMMUTABLE));
        }
        let contribution = self.contribution(PlanningPairRound::Clarification, slot, advice)?;
        let hash = contribution.document_hash.clone();
        if let Some(clarification) = &mut self.clarification {
            clarification.answers[slot.index()] = Some(contribution);
        }
        Ok(hash)
    }

    /// Record the caller's disposition. The pair then ends.
    ///
    /// # Errors
    /// [`DomainError::Terminal`] after an earlier disposition; a missing
    /// authority unless the actor is the caller; missing evidence until both
    /// findings and every requested answer are recorded; an empty rationale;
    /// a disposition that does not address `seat-a` then `seat-b` with each
    /// member's exact finding and answer; and a disposition that claims to
    /// supersede one.
    pub fn record_disposition(
        &mut self,
        actor: PlanningPairActor,
        disposition: PlanningPairDisposition,
    ) -> DomainResult<()> {
        self.open()?;
        Self::caller_only(actor)?;
        self.findings_complete()?;
        if self
            .clarification
            .as_ref()
            .is_some_and(|clarification| !clarification.is_answered())
        {
            return Err(DomainError::MissingEvidence {
                subject: "PlanningPairRun",
                rule: ANSWERS_INCOMPLETE,
            });
        }
        if disposition.rationale.as_str().trim().is_empty() {
            return Err(DomainError::invalid("PlanningPairRun", SAY_SOMETHING));
        }
        let slots: Vec<PlanningPairSlot> = disposition
            .members
            .iter()
            .map(|member| member.slot)
            .collect();
        if slots != PlanningPairSlot::ALL {
            return Err(DomainError::invalid("PlanningPairRun", DISSENT_LOST));
        }
        for member in &disposition.members {
            let index = member.slot.index();
            let finding = self.findings[index]
                .as_ref()
                .map(|finding| &finding.document_hash);
            let answer = self
                .clarification
                .as_ref()
                .and_then(|clarification| clarification.answers[index].as_ref())
                .map(|answer| &answer.document_hash);
            if finding != Some(&member.finding) || answer != member.answer.as_ref() {
                return Err(DomainError::invalid("PlanningPairRun", DISSENT_LOST));
            }
            if member.disposition == AdviceDisposition::Superseded {
                return Err(DomainError::invalid("PlanningPairRun", FIRST_DISPOSITION));
            }
        }
        self.disposition = Some(disposition);
        Ok(())
    }

    fn open(&self) -> DomainResult<()> {
        if self.disposition.is_some() {
            return Err(DomainError::Terminal {
                subject: "PlanningPairRun",
            });
        }
        Ok(())
    }

    fn member_only(actor: PlanningPairActor, slot: PlanningPairSlot) -> DomainResult<()> {
        if actor == PlanningPairActor::Member(slot) {
            Ok(())
        } else {
            Err(DomainError::MissingAuthority {
                subject: "PlanningPairRun",
                rule: MEMBER_ONLY,
            })
        }
    }

    fn caller_only(actor: PlanningPairActor) -> DomainResult<()> {
        if actor == PlanningPairActor::Caller {
            Ok(())
        } else {
            Err(DomainError::MissingAuthority {
                subject: "PlanningPairRun",
                rule: CALLER_ONLY,
            })
        }
    }

    fn findings_complete(&self) -> DomainResult<()> {
        if self.findings().is_none() {
            return Err(DomainError::MissingEvidence {
                subject: "PlanningPairRun",
                rule: FINDINGS_INCOMPLETE,
            });
        }
        Ok(())
    }

    fn contribution(
        &self,
        round: PlanningPairRound,
        slot: PlanningPairSlot,
        advice: BoundedText,
    ) -> DomainResult<PlanningPairContribution> {
        if advice.as_str().trim().is_empty() {
            return Err(DomainError::invalid("PlanningPairRun", SAY_SOMETHING));
        }
        let document = CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "protocol": PLANNING_PAIR_PROTOCOL,
            "spec_hash": self.spec_hash.as_str(),
            "placement_hash": self.members.placement_hash.as_str(),
            "round": round.as_str(),
            "slot": slot.as_str(),
            "advice": advice.as_str(),
        }))?;
        Ok(PlanningPairContribution {
            round,
            slot,
            advice,
            document_hash: document.hash().clone(),
        })
    }
}

impl PlanningPairClarification {
    fn is_answered(&self) -> bool {
        self.request
            .addressed
            .iter()
            .all(|slot| self.answers[slot.index()].is_some())
    }
}
