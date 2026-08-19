//! The open-question ledger: an unresolved ambiguity as a durable record.
//!
//! A seat that must proceed on an assumption it cannot evidence records the
//! ambiguity here instead of in its transcript. The reason is economic rather
//! than bureaucratic: an ambiguity that is not marked when it is found becomes
//! far more expensive once later work has been built on top of it, and a
//! transcript is not a place anything can be gated on.
//!
//! Three properties shape every type below.
//!
//! **History is append-only.** A round is never edited and a disposition is
//! never rewritten. A correction appends a successor that *names* what it
//! supersedes, so the record of having been wrong survives being corrected —
//! which is the only version of this ledger that is worth reading later.
//!
//! **Closing is a typed disposition, and there are exactly three.**
//! [`DispositionOutcome`] is closed: `resolved` names the deciding record,
//! `deferred` names the concrete trigger that must reopen the question, and
//! `not_relevant` carries its reason. There is deliberately no "closed" or
//! "wontfix" variant, because both are ways of ending a question without saying
//! what happened to it. An undispositioned question is not a valid end state,
//! and the completion gate in `kontor-policy` refuses `done` while one exists.
//!
//! **Authority is data, not a role literal.** This module never mentions a
//! deployment's role codes. A [`CloserPolicy`] carries the configured
//! architecture and process closers, and [`QuestionScope`] decides which of the
//! two a given question needs. Hard-coding the code of a role would make the
//! generic core branch on one deployment's roster — exactly what the crate
//! contract forbids.
//!
//! Nothing here escalates, notifies or scans. Reopening happens only through
//! [`OpenQuestion::fire_trigger`], and the report-only detector in
//! [`detect`](crate::open_question::detect) returns findings without touching a
//! single aggregate.

use crate::id::{
    AggregateRevision, BoundedText, ContentHash, MiniProjectId, OpenQuestionId, ProjectId, RoleKey,
    SeatBindingId, Timestamp, TriggerKey,
};
use crate::receipt::AggregateRef;
use crate::spec::{Shareability, ShareabilityTier};
use crate::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

/// Upper bound on the options one round may record.
///
/// Only the option list is bounded. Rounds, dispositions and firings each cost
/// an authorized command with its own receipt, so capping them would refuse a
/// legitimate long correction history rather than stop a runaway.
pub const MAX_QUESTION_OPTIONS: usize = 32;

crate::closed_enum! {
    /// What kind of question this is, and therefore who may close it.
    ///
    /// The scope is recorded when the question is raised, by the seat that
    /// tripped over the ambiguity. It is not derived from the attachment: the
    /// same decision record can host an architectural question and a routing
    /// one, and they do not have the same closer.
    QuestionScope, "QuestionScope" {
        /// A question about structure, boundaries or technical approach.
        Architecture => "architecture",
        /// A question about what the product should do.
        Product => "product",
        /// A question about how work is run.
        Process => "process",
        /// A question about who or what work goes to.
        Routing => "routing",
    }
}

impl QuestionScope {
    /// Whether this scope is closed by the architecture closer.
    ///
    /// The split is deliberately two-way rather than four-way: architectural
    /// and product questions are both about *what is built*, and process and
    /// routing questions are both about *how it is run*.
    #[must_use]
    pub const fn needs_architecture_closer(self) -> bool {
        matches!(self, Self::Architecture | Self::Product)
    }
}

crate::closed_enum! {
    /// The three ways a question may be closed, as a persisted discriminator.
    ///
    /// [`DispositionOutcome`] carries each variant's payload; this is the
    /// column SQL stores and checks. It exists separately so an unknown
    /// spelling arriving from the database is rejected rather than defaulted,
    /// and so no fourth way of closing a question can be added in one layer
    /// without the other refusing it.
    DispositionKind, "DispositionKind" {
        /// A deciding record now answers the question.
        Resolved => "resolved",
        /// The question is parked until a named trigger fires.
        Deferred => "deferred",
        /// The question turned out not to matter.
        NotRelevant => "not_relevant",
    }
}

/// The exact revision of a durable record, as a question cites it.
///
/// Both halves are mandatory. A citation naming only the record would still be
/// satisfied after that record was superseded, which is precisely the drift the
/// detector exists to find.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionCitation {
    /// The record that decides the question.
    pub record: AggregateRef,
    /// The exact revision of that record.
    pub revision: ContentHash,
}

/// What a question hangs off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenQuestionAttachment {
    /// A durable Kontor aggregate.
    Record(AggregateRef),
    /// A canonical document, addressed by its exact revision.
    Document(ContentHash),
}

/// The concrete event that must reopen a deferred question.
///
/// `condition` is prose for a human; `key` is the identity a firing matches on.
/// A deferral whose condition were only prose would be indistinguishable from
/// abandoning the question, so both are required and neither may be empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReopeningTrigger {
    /// The trigger's identity. A firing matches on exactly this.
    pub key: TriggerKey,
    /// What concretely has to happen, in one sentence.
    pub condition: BoundedText,
}

impl ReopeningTrigger {
    /// Prove the deferral names something that can actually fire.
    ///
    /// # Errors
    /// Refuses an empty condition — a deferral with no stated condition is an
    /// abandoned question wearing a smaller word.
    pub fn validate(&self) -> DomainResult<()> {
        if self.condition.as_str().trim().is_empty() {
            return Err(DomainError::invalid(
                "ReopeningTrigger",
                "a deferral must state the concrete condition that reopens it",
            ));
        }
        Ok(())
    }
}

/// One of exactly three ways to close a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispositionOutcome {
    /// A named record, at a named revision, decides it.
    Resolved(DecisionCitation),
    /// Parked until the named trigger fires.
    Deferred(ReopeningTrigger),
    /// It does not matter, for the stated reason.
    NotRelevant(BoundedText),
}

impl DispositionOutcome {
    /// The persisted discriminator for this outcome.
    #[must_use]
    pub const fn kind(&self) -> DispositionKind {
        match self {
            Self::Resolved(_) => DispositionKind::Resolved,
            Self::Deferred(_) => DispositionKind::Deferred,
            Self::NotRelevant(_) => DispositionKind::NotRelevant,
        }
    }

    /// The trigger this outcome parks on, when it is a deferral.
    #[must_use]
    pub const fn deferred_trigger(&self) -> Option<&ReopeningTrigger> {
        match self {
            Self::Deferred(trigger) => Some(trigger),
            Self::Resolved(_) | Self::NotRelevant(_) => None,
        }
    }

    /// Prove the payload is usable.
    ///
    /// # Errors
    /// Refuses a deferral with no concrete condition and a `not_relevant` with
    /// no stated reason. `resolved` needs no check beyond its types: both parts
    /// of a [`DecisionCitation`] are mandatory and already validated.
    pub fn validate(&self) -> DomainResult<()> {
        match self {
            Self::Resolved(_) => Ok(()),
            Self::Deferred(trigger) => trigger.validate(),
            Self::NotRelevant(reason) => {
                if reason.as_str().trim().is_empty() {
                    return Err(DomainError::invalid(
                        "DispositionOutcome",
                        "`not_relevant` must say why it is not relevant",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// One statement of why the state is ambiguous, and what the options were.
///
/// Rounds are ordinal-addressed and immutable. A later round may name the one
/// it supersedes; the predecessor's bytes never change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbiguityRound {
    /// One-based position in the append-only round history.
    pub ordinal: u32,
    /// The seat that recorded it.
    pub author: SeatBindingId,
    /// Why the state is ambiguous.
    pub why_ambiguous: BoundedText,
    /// The options seen, in the order they were seen. Never empty.
    pub options: Vec<BoundedText>,
    /// The round this one corrects, when it is a correction.
    pub supersedes: Option<u32>,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// One recorded closing of a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disposition {
    /// One-based position in the append-only disposition history.
    pub ordinal: u32,
    /// The seat that closed it.
    pub author: SeatBindingId,
    /// Which of the three closings this is.
    pub outcome: DispositionOutcome,
    /// The disposition this one replaces, when it is a correction.
    pub supersedes: Option<u32>,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// One recorded firing of a deferred question's trigger.
///
/// The firing names the disposition it fired *against*. Deciding whether a
/// question is reopened by comparing timestamps would be a bug waiting for two
/// records written in the same instant; naming the ordinal cannot be ambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerFiring {
    /// One-based position in the append-only firing history.
    pub ordinal: u32,
    /// The deferred disposition whose trigger fired.
    pub disposition_ordinal: u32,
    /// The trigger that fired.
    pub trigger: TriggerKey,
    /// The seat that observed it.
    pub observed_by: SeatBindingId,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

crate::closed_enum! {
    /// A question's current state, derived from its append-only history.
    ///
    /// This is never stored as the truth. It is computed from the rounds,
    /// dispositions and firings, so a stored status could not disagree with the
    /// history that produced it.
    OpenQuestionStatus, "OpenQuestionStatus" {
        /// Raised and not yet dispositioned. Blocks completion.
        Open => "open",
        /// A deciding record answers it.
        Resolved => "resolved",
        /// Parked on a trigger that has not fired.
        Deferred => "deferred",
        /// Recorded as not mattering.
        NotRelevant => "not_relevant",
        /// A deferral's trigger fired. Open again, and blocks completion.
        Reopened => "reopened",
    }
}

impl OpenQuestionStatus {
    /// Whether a question in this state stops its epic reaching `done`.
    ///
    /// Every disposition releases the gate; the absence of one, and a deferral
    /// whose trigger has since fired, do not.
    #[must_use]
    pub const fn blocks_completion(self) -> bool {
        matches!(self, Self::Open | Self::Reopened)
    }
}

/// Who may close which questions, as configuration rather than as code.
///
/// Both closers are role keys supplied by the deployment's domain pack. The
/// core never learns what they spell, which is what stops a generic aggregate
/// branching on one realm's roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloserPolicy {
    /// Closes architecture and product questions.
    pub architecture_closer: RoleKey,
    /// Closes process and routing questions.
    pub process_closer: RoleKey,
}

impl CloserPolicy {
    /// The role that may close questions of `scope`.
    #[must_use]
    pub const fn closer_for(&self, scope: QuestionScope) -> &RoleKey {
        if scope.needs_architecture_closer() {
            &self.architecture_closer
        } else {
            &self.process_closer
        }
    }
}

/// One question's identity and derived state, as a gate reads it.
///
/// The completion predicate needs the identity and the subject and nothing
/// else, so this is what crosses into `kontor-policy` rather than the whole
/// history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenQuestionSummary {
    /// The question.
    pub question_id: OpenQuestionId,
    /// What it is about.
    pub subject: BoundedText,
    /// Its derived state.
    pub status: OpenQuestionStatus,
}

/// One durable open question: an immutable header over append-only history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenQuestion {
    /// The question.
    pub question_id: OpenQuestionId,
    /// The project it belongs to. Every read is scoped by this.
    pub project_id: ProjectId,
    /// The epic it is attached to.
    pub mini_project_id: MiniProjectId,
    /// What it is about.
    pub subject: BoundedText,
    /// Which closer it needs.
    pub scope: QuestionScope,
    /// The record or document it hangs off.
    pub attachment: OpenQuestionAttachment,
    /// The seat that raised it.
    pub author: SeatBindingId,
    /// The write-time classification. Never reclassified.
    pub shareability: Shareability,
    /// When it was raised.
    pub created_at: Timestamp,
    /// Compare-and-swap revision of the header.
    pub revision: AggregateRevision,
    /// Append-only rounds, in ordinal order.
    pub rounds: Vec<AmbiguityRound>,
    /// Append-only dispositions, in ordinal order.
    pub dispositions: Vec<Disposition>,
    /// Append-only trigger firings, in ordinal order.
    pub firings: Vec<TriggerFiring>,
}

impl OpenQuestion {
    /// Raise a question.
    ///
    /// Any valid seat may raise one. There is deliberately no role or
    /// capability check here: the seat that trips over an ambiguity is the only
    /// one that knows it did, and a permission check on *reporting* a problem
    /// would only ever suppress reports.
    ///
    /// The classification is the tier default rather than a caller's choice.
    /// An open question is project knowledge, so it is `project_shared`, and no
    /// method on this type reclassifies it.
    ///
    /// # Errors
    /// Refuses an empty subject and an unusable first round.
    #[allow(clippy::too_many_arguments)]
    pub fn raise(
        question_id: OpenQuestionId,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
        subject: BoundedText,
        scope: QuestionScope,
        attachment: OpenQuestionAttachment,
        author: SeatBindingId,
        why_ambiguous: BoundedText,
        options: Vec<BoundedText>,
        now: Timestamp,
    ) -> DomainResult<Self> {
        if subject.as_str().trim().is_empty() {
            return Err(DomainError::invalid(
                "OpenQuestion",
                "a question must say what it is about",
            ));
        }
        validate_round(&why_ambiguous, &options)?;
        Ok(Self {
            question_id,
            project_id,
            mini_project_id,
            subject,
            scope,
            attachment,
            author,
            shareability: Shareability::default_for(ShareabilityTier::ProjectKnowledge)?,
            created_at: now,
            revision: AggregateRevision::INITIAL,
            rounds: vec![AmbiguityRound {
                ordinal: 1,
                author,
                why_ambiguous,
                options,
                supersedes: None,
                recorded_at: now,
            }],
            dispositions: Vec::new(),
            firings: Vec::new(),
        })
    }

    /// The current disposition: the last one appended.
    ///
    /// Latest-wins is the whole rule. `supersedes` records *what* a correction
    /// replaced, for anyone reading the history; it is not consulted here,
    /// because a correction that had to be found by graph traversal would make
    /// the current state depend on how carefully every author filled in a
    /// back-reference.
    #[must_use]
    pub fn current_disposition(&self) -> Option<&Disposition> {
        self.dispositions.last()
    }

    /// The question's state, derived from its history.
    #[must_use]
    pub fn status(&self) -> OpenQuestionStatus {
        let Some(current) = self.current_disposition() else {
            return OpenQuestionStatus::Open;
        };
        match &current.outcome {
            DispositionOutcome::Resolved(_) => OpenQuestionStatus::Resolved,
            DispositionOutcome::NotRelevant(_) => OpenQuestionStatus::NotRelevant,
            DispositionOutcome::Deferred(trigger) => {
                if self.fired_against(current.ordinal, &trigger.key) {
                    OpenQuestionStatus::Reopened
                } else {
                    OpenQuestionStatus::Deferred
                }
            }
        }
    }

    /// Identity, subject and derived state, for the completion gate.
    #[must_use]
    pub fn summary(&self) -> OpenQuestionSummary {
        OpenQuestionSummary {
            question_id: self.question_id,
            subject: self.subject.clone(),
            status: self.status(),
        }
    }

    /// Append a correcting round.
    ///
    /// The predecessor is untouched. `supersedes` must name a round that
    /// exists, so a correction cannot point at nothing.
    ///
    /// # Errors
    /// Refuses an unusable round, an unknown predecessor and an ordinal
    /// overflow.
    pub fn append_round(
        &mut self,
        author: SeatBindingId,
        why_ambiguous: BoundedText,
        options: Vec<BoundedText>,
        supersedes: Option<u32>,
        now: Timestamp,
    ) -> DomainResult<u32> {
        validate_round(&why_ambiguous, &options)?;
        if supersedes.is_some_and(|predecessor| {
            !self.rounds.iter().any(|round| round.ordinal == predecessor)
        }) {
            return Err(DomainError::invalid(
                "OpenQuestion round",
                "a correction must name a round that exists",
            ));
        }
        let ordinal = next_ordinal(self.rounds.len(), "OpenQuestion round")?;
        self.rounds.push(AmbiguityRound {
            ordinal,
            author,
            why_ambiguous,
            options,
            supersedes,
            recorded_at: now,
        });
        self.revision = self.revision.next()?;
        Ok(ordinal)
    }

    /// Close the question, or correct how it was closed.
    ///
    /// Authority is checked against `policy`, never against a role literal.
    ///
    /// # Errors
    /// Refuses an outcome whose payload is unusable, an author whose role is
    /// not the configured closer for this question's scope, an unknown
    /// superseded ordinal, and an ordinal overflow.
    pub fn dispose(
        &mut self,
        author: SeatBindingId,
        author_role: &RoleKey,
        policy: &CloserPolicy,
        outcome: DispositionOutcome,
        supersedes: Option<u32>,
        now: Timestamp,
    ) -> DomainResult<u32> {
        outcome.validate()?;
        if author_role != policy.closer_for(self.scope) {
            return Err(DomainError::MissingAuthority {
                subject: "OpenQuestion disposition",
                rule: "only the configured closer for this question's scope may close it",
            });
        }
        if supersedes.is_some_and(|predecessor| {
            !self
                .dispositions
                .iter()
                .any(|disposition| disposition.ordinal == predecessor)
        }) {
            return Err(DomainError::invalid(
                "OpenQuestion disposition",
                "a correction must name a disposition that exists",
            ));
        }
        let ordinal = next_ordinal(self.dispositions.len(), "OpenQuestion disposition")?;
        self.dispositions.push(Disposition {
            ordinal,
            author,
            outcome,
            supersedes,
            recorded_at: now,
        });
        self.revision = self.revision.next()?;
        Ok(ordinal)
    }

    /// Record that the current deferral's trigger fired, reopening the question.
    ///
    /// The deferred disposition is neither deleted nor rewritten: the question
    /// becomes open again because a firing now stands against that disposition,
    /// and the history still shows it was deferred and why.
    ///
    /// # Errors
    /// Refuses a question that is not currently deferred, a trigger that is not
    /// the one the current deferral named, a firing that has already been
    /// recorded against that deferral, and an ordinal overflow.
    pub fn fire_trigger(
        &mut self,
        trigger: &TriggerKey,
        observed_by: SeatBindingId,
        now: Timestamp,
    ) -> DomainResult<u32> {
        let current = self.current_disposition().ok_or(DomainError::invalid(
            "OpenQuestion trigger",
            "an undispositioned question has no deferral to reopen",
        ))?;
        let disposition_ordinal = current.ordinal;
        let deferred = current
            .outcome
            .deferred_trigger()
            .ok_or(DomainError::invalid(
                "OpenQuestion trigger",
                "only a deferred question reopens on a trigger",
            ))?;
        if &deferred.key != trigger {
            return Err(DomainError::invalid(
                "OpenQuestion trigger",
                "only the exact trigger the deferral named reopens it",
            ));
        }
        if self.fired_against(disposition_ordinal, trigger) {
            return Err(DomainError::invalid(
                "OpenQuestion trigger",
                "this deferral's trigger has already been recorded as fired",
            ));
        }
        let ordinal = next_ordinal(self.firings.len(), "OpenQuestion trigger")?;
        self.firings.push(TriggerFiring {
            ordinal,
            disposition_ordinal,
            trigger: trigger.clone(),
            observed_by,
            recorded_at: now,
        });
        self.revision = self.revision.next()?;
        Ok(ordinal)
    }

    /// Whether `trigger` has fired against the disposition at `ordinal`.
    fn fired_against(&self, ordinal: u32, trigger: &TriggerKey) -> bool {
        self.firings
            .iter()
            .any(|firing| firing.disposition_ordinal == ordinal && &firing.trigger == trigger)
    }
}

/// Validate one round's prose and option set.
fn validate_round(why_ambiguous: &BoundedText, options: &[BoundedText]) -> DomainResult<()> {
    const SUBJECT: &str = "OpenQuestion round";
    if why_ambiguous.as_str().trim().is_empty() {
        return Err(DomainError::invalid(
            SUBJECT,
            "a round must say why the state is ambiguous",
        ));
    }
    if options.is_empty() {
        return Err(DomainError::invalid(
            SUBJECT,
            "a round must record the options that were seen",
        ));
    }
    if options.len() > MAX_QUESTION_OPTIONS {
        return Err(DomainError::invalid(
            SUBJECT,
            "records more options than one round may hold",
        ));
    }
    if options
        .iter()
        .any(|option| option.as_str().trim().is_empty())
    {
        return Err(DomainError::invalid(
            SUBJECT,
            "an option must say something",
        ));
    }
    if options
        .iter()
        .enumerate()
        .any(|(index, option)| options[index + 1..].contains(option))
    {
        return Err(DomainError::invalid(SUBJECT, "records one option twice"));
    }
    Ok(())
}

/// The next one-based ordinal for an append-only child list.
fn next_ordinal(current_len: usize, subject: &'static str) -> DomainResult<u32> {
    u32::try_from(current_len)
        .ok()
        .and_then(|len| len.checked_add(1))
        .ok_or_else(|| DomainError::invalid(subject, "the append-only history overflowed"))
}

// ---------------------------------------------------------------------------
// The report-only detector pass
// ---------------------------------------------------------------------------

/// One accepted decision, as the detector observes it.
///
/// `superseded` is the observation's own fact, not something inferred here: the
/// detector is given what the store already knows and is not allowed to go and
/// look, which is what keeps it a pure function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedDecision {
    /// What this decision decides.
    pub subject: BoundedText,
    /// The deciding record.
    pub record: AggregateRef,
    /// Its exact revision.
    pub revision: ContentHash,
    /// Whether that exact revision has been superseded.
    pub superseded: bool,
}

/// Everything the detector is allowed to see.
///
/// Every field is a shared borrow. There is no repository, no mutable aggregate
/// and no command port in this type, so a detector that wanted to resolve a
/// question could not reach anything to do it with — the read-only boundary
/// starts by being unrepresentable rather than by being remembered.
#[derive(Debug, Clone, Copy)]
pub struct DetectorObservations<'a> {
    /// The project's questions.
    pub questions: &'a [OpenQuestion],
    /// Currently accepted decisions.
    pub decisions: &'a [AcceptedDecision],
    /// Triggers observed to have fired.
    pub fired_triggers: &'a [TriggerKey],
}

/// One machine-checkable ambiguity the detector found.
///
/// A finding is a report. It is never a state change: nothing in this enum can
/// be applied, and the only way a question's state moves is an explicit
/// authorized command on the aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenQuestionFinding {
    /// Two or more accepted decisions about one subject disagree.
    ContradictingDecisions {
        /// The contested subject.
        subject: BoundedText,
        /// The distinct accepted revisions, in ascending order.
        revisions: Vec<ContentHash>,
    },
    /// A question was resolved by citing a revision that is now superseded.
    SupersededCitation {
        /// The question.
        question_id: OpenQuestionId,
        /// What it is about.
        subject: BoundedText,
        /// The superseded revision it cites.
        revision: ContentHash,
    },
    /// A deferred question's named trigger has fired, but no firing is recorded.
    ///
    /// Reported rather than applied. Recording the firing is
    /// [`OpenQuestion::fire_trigger`] — an authorized command with a receipt —
    /// and a detector that reopened the question itself would be deciding, on
    /// the strength of one observation, that a deferral was over.
    DeferredTriggerFired {
        /// The question.
        question_id: OpenQuestionId,
        /// What it is about.
        subject: BoundedText,
        /// The trigger that fired.
        trigger: TriggerKey,
    },
}

/// Report the machine-checkable subset of ambiguity. Changes nothing.
///
/// The order is stable and independent of the order the observations arrive in:
/// contradiction findings come first in subject order, then per-question
/// findings in question-id order. A caller diffing two runs is comparing
/// findings, not input shuffling.
#[must_use]
pub fn detect(observations: &DetectorObservations<'_>) -> Vec<OpenQuestionFinding> {
    let mut findings = Vec::new();

    // Contradiction: among decisions still accepted, one subject carrying more
    // than one distinct revision. A superseded revision is excluded, because
    // being replaced is the opposite of a contradiction.
    let mut by_subject: std::collections::BTreeMap<
        &BoundedText,
        std::collections::BTreeSet<&ContentHash>,
    > = std::collections::BTreeMap::new();
    for decision in observations.decisions {
        if !decision.superseded {
            by_subject
                .entry(&decision.subject)
                .or_default()
                .insert(&decision.revision);
        }
    }
    for (subject, revisions) in by_subject {
        if revisions.len() > 1 {
            findings.push(OpenQuestionFinding::ContradictingDecisions {
                subject: subject.clone(),
                revisions: revisions.into_iter().cloned().collect(),
            });
        }
    }

    // Per-question findings, in question-id order.
    let mut ordered: Vec<&OpenQuestion> = observations.questions.iter().collect();
    ordered.sort_by_key(|question| question.question_id);
    for question in ordered {
        let Some(current) = question.current_disposition() else {
            continue;
        };
        match &current.outcome {
            DispositionOutcome::Resolved(citation) => {
                let superseded = observations.decisions.iter().any(|decision| {
                    decision.record == citation.record
                        && decision.revision == citation.revision
                        && decision.superseded
                });
                if superseded {
                    findings.push(OpenQuestionFinding::SupersededCitation {
                        question_id: question.question_id,
                        subject: question.subject.clone(),
                        revision: citation.revision.clone(),
                    });
                }
            }
            DispositionOutcome::Deferred(trigger) => {
                // A question already reopened needs no report: its status
                // already says so, and the firing is already durable.
                let already_recorded = question.status() == OpenQuestionStatus::Reopened;
                if !already_recorded && observations.fired_triggers.contains(&trigger.key) {
                    findings.push(OpenQuestionFinding::DeferredTriggerFired {
                        question_id: question.question_id,
                        subject: question.subject.clone(),
                        trigger: trigger.key.clone(),
                    });
                }
            }
            DispositionOutcome::NotRelevant(_) => {}
        }
    }

    findings
}
