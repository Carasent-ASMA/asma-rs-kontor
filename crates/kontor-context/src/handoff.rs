//! Same-engine continuation metadata and portable cross-engine handoffs.
//!
//! These are two different things and the type system says so.
//!
//! [`SameEngineContinuation`] is an opaque locator for *one* runtime generation.
//! It only ever resumes the native session it names, it is validated against the
//! parent run and the original pack digest, and it appears nowhere in the
//! portable capsule.
//!
//! [`HandoffCapsule`] is the portable artefact. It carries explicit, reviewable
//! work evidence — workspace, files, commits, tests, decisions, evidence,
//! remaining work, risks and a recommended next action — and it carries no
//! transcript, no hidden model state, no token cache, no credentials and no
//! provider session locator. Kontor does not claim continuity it cannot deliver.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use kontor_core::id::{
    AgentRunId, BoundedText, CanonicalDocument, CommandReceiptId, ContentHash, ContextPackId,
    ExternalId, HandoffId, RealmId, SCHEMA_VERSION, SchemaVersion, Timestamp,
};
use kontor_core::realm::ensure_realm;
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::{DomainError, DomainResult};

use crate::model::WorkspaceRef;

/// How a successor run continues its predecessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationMode {
    /// The same runtime generation resumes its own native session.
    SameEngine,
    /// A different engine starts fresh from a portable capsule.
    CrossEngineHandoff,
}

/// The locator that lets one runtime generation resume its own session.
///
/// This is deliberately *not* portable context: it names a native session inside
/// one generation of one runtime, and it is worthless — and refused — anywhere
/// else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SameEngineContinuation {
    /// The envelope contract this record was written under.
    pub schema_version: SchemaVersion,
    /// The realm the parent run belongs to.
    pub realm_id: RealmId,
    /// The run being continued.
    pub parent_run_id: AgentRunId,
    /// The native session, qualified by runtime kind, host and generation.
    pub native_identity: NativeRuntimeIdentity,
    /// The dispatcher correlation recorded with the parent run.
    pub correlation: ExternalId,
    /// Digest of the stored evidence this locator was captured from.
    pub evidence_hash: ContentHash,
    /// The digest of the Context Pack the parent run was frozen against.
    pub context_pack_hash: ContentHash,
}

impl SameEngineContinuation {
    /// Prove this locator may resume `parent_run_id` on the `observed` session
    /// under `context_pack_hash`.
    ///
    /// # Errors
    /// * [`DomainError::Invalid`] for a foreign envelope contract, a different
    ///   parent run, or a different original pack digest.
    /// * [`DomainError::RealmMismatch`] for another realm.
    /// * [`DomainError::MissingAuthority`] when the observed session is a
    ///   different runtime engine, a different generation of the same runtime, or
    ///   simply a different session. A provider name is not a resumable binding.
    pub fn ensure_resumable(
        &self,
        realm_id: RealmId,
        parent_run_id: AgentRunId,
        observed: &NativeRuntimeIdentity,
        context_pack_hash: &ContentHash,
    ) -> DomainResult<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(DomainError::invalid(
                "SameEngineContinuation",
                "was written under a different envelope contract",
            ));
        }
        ensure_realm(realm_id, self.realm_id)?;
        if self.parent_run_id != parent_run_id {
            return Err(DomainError::invalid(
                "SameEngineContinuation",
                "names a different parent run",
            ));
        }
        if self.native_identity.runtime_kind != observed.runtime_kind {
            return Err(DomainError::MissingAuthority {
                subject: "same-engine continuation",
                rule: "the observed session belongs to a different runtime engine",
            });
        }
        if self.native_identity.generation_changed(observed) {
            return Err(DomainError::MissingAuthority {
                subject: "same-engine continuation",
                rule: "the observed session belongs to a different runtime generation",
            });
        }
        if !self.native_identity.same_session(observed) {
            return Err(DomainError::MissingAuthority {
                subject: "same-engine continuation",
                rule: "the observed session is not the session this locator names",
            });
        }
        if &self.context_pack_hash != context_pack_hash {
            return Err(DomainError::invalid(
                "SameEngineContinuation",
                "was captured against a different Context Pack digest",
            ));
        }
        Ok(())
    }
}

/// What one recorded test attempt concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestResult {
    /// The command ran and passed.
    Passed,
    /// The command ran and failed.
    Failed,
    /// The command was not run.
    Skipped,
}

/// One test command and what it concluded.
///
/// The result is part of the entry, so re-running the same command after a fix
/// is two distinct, ordered attempts rather than a rejected duplicate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestAttempt {
    /// The command that was run, verbatim.
    pub command: BoundedText,
    /// What it concluded.
    pub result: TestResult,
}

/// The portable, cross-engine handoff.
///
/// Every list is required and ordered. A producer with nothing to report in a
/// category sends an explicit empty list; omitting the category is a rejected
/// document, not an empty one, so "no tests" and "forgot the tests" are never
/// the same capsule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffCapsule {
    /// The envelope contract this capsule was written under.
    pub schema_version: SchemaVersion,
    /// The realm both runs belong to.
    pub realm_id: RealmId,
    /// This handoff's identity.
    pub handoff_id: HandoffId,
    /// Always [`ContinuationMode::CrossEngineHandoff`]; anything else rejects.
    pub continuation_mode: ContinuationMode,
    /// The run handing over.
    pub source_run_id: AgentRunId,
    /// The run taking over, once it exists.
    pub target_run_id: Option<AgentRunId>,
    /// The Context Pack the source run was frozen against.
    pub context_pack_id: ContextPackId,
    /// That pack's digest.
    pub context_pack_hash: ContentHash,
    /// Workspace root, branch and baseline the successor inherits.
    pub workspace: WorkspaceRef,
    /// What the source run attempted, in order.
    pub attempted_work: Vec<BoundedText>,
    /// Files the source run touched.
    pub touched_files: Vec<BoundedText>,
    /// Commits the source run produced.
    pub commits: Vec<ExternalId>,
    /// Test commands and their results, in order.
    pub tests: Vec<TestAttempt>,
    /// Decisions taken, so the successor does not relitigate them.
    pub decisions: Vec<BoundedText>,
    /// References to stored evidence.
    pub evidence: Vec<ExternalId>,
    /// What is left to do.
    pub remaining_work: Vec<BoundedText>,
    /// Known risks.
    pub risks: Vec<BoundedText>,
    /// The single recommended next action.
    pub recommended_next_action: BoundedText,
}

impl HandoffCapsule {
    /// Validate the capsule without canonicalizing it.
    ///
    /// # Errors
    /// * [`DomainError::Invalid`] for a foreign envelope contract, a capsule
    ///   claiming same-engine continuation, an exact duplicate entry in any
    ///   list, a handoff to itself, or an empty recommended next action.
    /// * [`DomainError::RealmMismatch`] for another realm.
    pub fn validate(&self, realm_id: RealmId) -> DomainResult<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(DomainError::invalid(
                "HandoffCapsule",
                "was written under a different envelope contract",
            ));
        }
        ensure_realm(realm_id, self.realm_id)?;
        if self.continuation_mode != ContinuationMode::CrossEngineHandoff {
            return Err(DomainError::invalid(
                "HandoffCapsule",
                "is portable context and may only claim cross-engine continuation",
            ));
        }
        if self.target_run_id == Some(self.source_run_id) {
            return Err(DomainError::invalid(
                "HandoffCapsule",
                "hands over to the run it comes from",
            ));
        }
        if self.recommended_next_action.as_str().trim().is_empty() {
            return Err(DomainError::invalid(
                "HandoffCapsule",
                "must recommend a next action",
            ));
        }
        reject_duplicates("HandoffCapsule.attempted_work", &self.attempted_work)?;
        reject_duplicates("HandoffCapsule.touched_files", &self.touched_files)?;
        reject_duplicates("HandoffCapsule.commits", &self.commits)?;
        reject_duplicates("HandoffCapsule.tests", &self.tests)?;
        reject_duplicates("HandoffCapsule.decisions", &self.decisions)?;
        reject_duplicates("HandoffCapsule.evidence", &self.evidence)?;
        reject_duplicates("HandoffCapsule.remaining_work", &self.remaining_work)?;
        reject_duplicates("HandoffCapsule.risks", &self.risks)
    }

    /// Validate and canonicalize the capsule.
    ///
    /// The returned document's digest is the capsule hash a receiver
    /// acknowledges; the core sensitive scanner runs over every string in it, so
    /// free prose and evidence metadata cannot become a side channel.
    ///
    /// # Errors
    /// As [`HandoffCapsule::validate`], plus anything
    /// [`CanonicalDocument::from_serializable`] returns.
    pub fn canonical(&self, realm_id: RealmId) -> DomainResult<CanonicalDocument> {
        self.validate(realm_id)?;
        CanonicalDocument::from_serializable(self)
    }
}

/// A receiver's explicit acknowledgement of one exact capsule.
///
/// It is a separate document bound to the capsule digest, not a flag on the
/// capsule: a receiver cannot acknowledge "a handoff", only *this* one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffAcknowledgement {
    /// The envelope contract this acknowledgement was written under.
    pub schema_version: SchemaVersion,
    /// The realm the capsule and both runs belong to.
    pub realm_id: RealmId,
    /// The digest of the exact capsule being acknowledged.
    pub capsule_hash: ContentHash,
    /// The run taking the work over.
    pub receiver_run_id: AgentRunId,
    /// The command receipt that recorded the acknowledgement.
    pub receipt_id: CommandReceiptId,
    /// Digest of the stored acknowledgement evidence.
    pub evidence_hash: ContentHash,
    /// When it was acknowledged.
    pub acknowledged_at: Timestamp,
}

impl HandoffAcknowledgement {
    /// Prove this acknowledgement is bound to `capsule`.
    ///
    /// # Errors
    /// * [`DomainError::Invalid`] for a foreign envelope contract or a digest
    ///   that is not this capsule's.
    /// * [`DomainError::RealmMismatch`] for another realm.
    pub fn ensure_acknowledges(
        &self,
        realm_id: RealmId,
        capsule: &CanonicalDocument,
    ) -> DomainResult<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(DomainError::invalid(
                "HandoffAcknowledgement",
                "was written under a different envelope contract",
            ));
        }
        ensure_realm(realm_id, self.realm_id)?;
        if &self.capsule_hash != capsule.hash() {
            return Err(DomainError::invalid(
                "HandoffAcknowledgement",
                "is bound to a different capsule digest",
            ));
        }
        Ok(())
    }

    /// Canonicalize the acknowledgement.
    ///
    /// # Errors
    /// Anything [`CanonicalDocument::from_serializable`] returns.
    pub fn canonical(&self) -> DomainResult<CanonicalDocument> {
        CanonicalDocument::from_serializable(self)
    }
}

/// Acknowledge one sealed capsule.
///
/// The capsule is re-read from its canonical bytes and re-validated here, so an
/// acknowledgement can only ever be produced for a capsule that is itself still
/// admissible in this realm — an unrecognized field in those bytes rejects the
/// import rather than being dropped on the floor.
///
/// `target_run_id` stays optional because a capsule is usually written before its
/// successor exists. But once a capsule *does* name its target, only that run may
/// acknowledge it: a capsule addressed to one run and acknowledged by another is
/// a contradiction, not a handoff.
///
/// # Errors
/// * [`DomainError::Invalid`] when the document is not a capsule, carries an
///   unknown field, is acknowledged by the run that produced it, or is
///   acknowledged by a run other than the target it names.
/// * Anything [`HandoffCapsule::validate`] returns.
pub fn acknowledge(
    realm_id: RealmId,
    capsule: &CanonicalDocument,
    receiver_run_id: AgentRunId,
    receipt_id: CommandReceiptId,
    evidence_hash: ContentHash,
    acknowledged_at: Timestamp,
) -> DomainResult<HandoffAcknowledgement> {
    let parsed: HandoffCapsule = capsule.deserialize()?;
    parsed.validate(realm_id)?;
    if parsed.source_run_id == receiver_run_id {
        return Err(DomainError::invalid(
            "HandoffAcknowledgement",
            "the run that produced the capsule cannot acknowledge it",
        ));
    }
    if parsed
        .target_run_id
        .is_some_and(|target| target != receiver_run_id)
    {
        return Err(DomainError::invalid(
            "HandoffAcknowledgement",
            "the capsule names a different target run",
        ));
    }
    Ok(HandoffAcknowledgement {
        schema_version: SCHEMA_VERSION,
        realm_id,
        capsule_hash: capsule.hash().clone(),
        receiver_run_id,
        receipt_id,
        evidence_hash,
        acknowledged_at,
    })
}

/// Reject an exact duplicate entry in a handoff category.
fn reject_duplicates<T: Ord>(subject: &'static str, entries: &[T]) -> DomainResult<()> {
    let mut seen: BTreeSet<&T> = BTreeSet::new();
    for entry in entries {
        if !seen.insert(entry) {
            return Err(DomainError::invalid(
                subject,
                "contains an exact duplicate entry",
            ));
        }
    }
    Ok(())
}
