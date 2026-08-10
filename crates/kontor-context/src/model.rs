//! Versioned Context Pack, provenance, redaction and run-binding types.
//!
//! Every identifier, hash, canonical document, realm check and sensitive-text
//! rule here is [`kontor_core`]'s. This module adds exactly one thing the core
//! does not have: the closed, ranked list of context layers and the metadata
//! that records which of them won.

use serde::{Deserialize, Serialize};

use kontor_core::id::{
    AgentRunId, BoundedText, CanonicalDocument, ContentHash, ContextPackId, ExternalId,
    ExternalName, RealmId, SCHEMA_VERSION, SchemaVersion, SpecVersion, Timestamp,
    reject_sensitive_text, validate_open_key,
};
use kontor_core::realm::ensure_realm;
use kontor_core::spec::JsonPointer;
use kontor_core::{DomainError, DomainResult};

/// Where a context source sits in the approved resolution order.
///
/// The order is architecture §11 and is closed: a caller supplies a layer, never
/// a numeric priority, so no deployment can reorder precedence from data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextLayer {
    /// Workspace-wide defaults.
    GlobalProfile,
    /// Project defaults.
    ProjectProfile,
    /// The mini-project or research scope the work belongs to.
    Scope,
    /// The profile of the team role the run executes as.
    TeamRoleProfile,
    /// Additions declared on the task itself.
    TaskAdditions,
    /// An explicit override supplied for this one run.
    RunOverride,
}

impl ContextLayer {
    /// Every layer, weakest first.
    pub const ALL: &'static [Self] = &[
        Self::GlobalProfile,
        Self::ProjectProfile,
        Self::Scope,
        Self::TeamRoleProfile,
        Self::TaskAdditions,
        Self::RunOverride,
    ];

    /// This layer's precedence rank: a higher rank wins a conflicting path.
    ///
    /// The ranks are written out rather than derived from declaration order so
    /// that swapping two of them is a visible, single-line change that the
    /// precedence tests fail on.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::GlobalProfile => 0,
            Self::ProjectProfile => 1,
            Self::Scope => 2,
            Self::TeamRoleProfile => 3,
            Self::TaskAdditions => 4,
            Self::RunOverride => 5,
        }
    }
}

/// Why a path was removed from a resolved pack.
///
/// A reason code is metadata: it is recorded in the redaction report while the
/// value it describes is gone from the pack, its hash input and its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionReason {
    /// The value carries, or may carry, credential material.
    CredentialLike,
    /// The value carries personal data.
    PersonalData,
    /// The value is authorized for a narrower scope than this pack.
    RestrictedScope,
    /// Policy excludes the value from portable context.
    PolicyExcluded,
}

/// A reference whose value is supplied by the caller rather than by the source.
///
/// The source declares *where* the value belongs and *which* grant authorizes
/// it. Admission happens before merging, so an unresolved, denied or
/// foreign-realm reference rejects the whole resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestrictedReference {
    /// Where the resolved value is written inside the source content.
    pub path: JsonPointer,
    /// The grant key the caller resolves this reference through.
    pub reference_key: String,
    /// The realm the reference resolves in.
    ///
    /// It must be the realm the pack is being resolved for. A declaration
    /// naming another realm is refused even when the caller holds a matching
    /// grant: authorization issued in Realm B is not authorization to carry a
    /// value into a Realm A pack.
    pub realm_id: RealmId,
}

impl RestrictedReference {
    /// Validate the declaration itself (never its value) against the realm the
    /// pack is being resolved for.
    ///
    /// # Errors
    /// * [`DomainError::RealmMismatch`] when the declaration resolves in another
    ///   realm than the pack.
    /// * [`DomainError::Invalid`] for a malformed grant key.
    /// * [`DomainError::SensitiveMaterial`] when the declared path is itself
    ///   credential-bearing.
    pub fn validate(&self, realm_id: RealmId) -> DomainResult<()> {
        ensure_realm(realm_id, self.realm_id)?;
        validate_open_key("RestrictedReference.reference_key", &self.reference_key)?;
        reject_sensitive_text("RestrictedReference.path", self.path.as_str())
    }
}

/// How the caller resolved one restricted reference.
///
/// A missing map entry is a *third* outcome — unresolved — and is rejected just
/// as firmly as a denial.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ResolvedReference {
    /// The grant was allowed and carries the value to inject.
    Allowed {
        /// The realm the grant was issued in.
        realm_id: RealmId,
        /// The authorized value.
        value: serde_json::Value,
    },
    /// The grant exists and was refused.
    Denied,
}

/// The grants a caller supplies for one resolution, keyed by grant key.
pub type ReferenceInputs = std::collections::BTreeMap<String, ResolvedReference>;

/// An explicit instruction to drop a path from the resolved pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionRule {
    /// The object member to remove, as an RFC 6901 pointer.
    pub path: JsonPointer,
    /// Why it is removed.
    pub reason: RedactionReason,
}

impl RedactionRule {
    /// Validate the declaration itself.
    ///
    /// # Errors
    /// Returns [`DomainError::SensitiveMaterial`] when the declared path is
    /// itself credential-bearing.
    pub fn validate(&self) -> DomainResult<()> {
        reject_sensitive_text("RedactionRule.path", self.path.as_str())
    }
}

/// One declared input to a Context Pack resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSource {
    /// The envelope contract this source was written under.
    pub schema_version: SchemaVersion,
    /// The realm the source belongs to.
    pub realm_id: RealmId,
    /// Which layer of the resolution order it contributes to.
    pub layer: ContextLayer,
    /// The stable source key, unique *within its layer*, and the secondary sort
    /// key so collection order cannot affect the result.
    ///
    /// The same key in two different layers is legal and expected — one profile
    /// key can contribute at several ranks — and `(layer, source_id)` is both the
    /// uniqueness key and the total ordering key, so two admitted sources never
    /// tie.
    pub source_id: String,
    /// The immutable revision of the source that was read.
    pub revision: SpecVersion,
    /// References the caller must resolve before this source may merge.
    pub restricted_references: Vec<RestrictedReference>,
    /// Paths this source declares must not survive into the resolved pack.
    pub redactions: Vec<RedactionRule>,
    /// The source's own contribution. Must be a JSON object.
    pub content: serde_json::Value,
}

impl ContextSource {
    /// Validate the source's envelope, identity and declarations.
    ///
    /// # Errors
    /// * [`DomainError::Invalid`] for a foreign envelope contract, a malformed
    ///   source key or non-object content.
    /// * [`DomainError::RealmMismatch`] for a source from another realm, or for a
    ///   restricted reference that resolves in another realm.
    /// * Anything [`RestrictedReference::validate`] or [`RedactionRule::validate`]
    ///   returns.
    pub fn validate(&self, realm_id: RealmId) -> DomainResult<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(DomainError::invalid(
                "ContextSource",
                "was written under a different envelope contract",
            ));
        }
        ensure_realm(realm_id, self.realm_id)?;
        validate_open_key("ContextSource.source_id", &self.source_id)?;
        if !self.content.is_object() {
            return Err(DomainError::invalid(
                "ContextSource",
                "content must be a JSON object",
            ));
        }
        for reference in &self.restricted_references {
            reference.validate(realm_id)?;
        }
        for redaction in &self.redactions {
            redaction.validate()?;
        }
        Ok(())
    }

    /// The ordering key: precedence rank first, stable source key second.
    #[must_use]
    pub fn order_key(&self) -> (u8, &str) {
        (self.layer.rank(), self.source_id.as_str())
    }
}

/// Which source won one resolved leaf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceEntry {
    /// The resolved leaf, as an RFC 6901 pointer into the pack.
    pub path: JsonPointer,
    /// The layer the winning source sits in.
    pub layer: ContextLayer,
    /// The winning source key.
    pub source_id: String,
    /// The revision of that source.
    pub revision: SpecVersion,
}

/// One applied redaction, recorded as metadata only.
///
/// There is deliberately no value, no hash of the value and no length: the
/// report says *that* a path was dropped and why, never what was there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionRecord {
    /// The path that was removed.
    pub path: JsonPointer,
    /// The source that declared the rule.
    pub source_id: String,
    /// The declared reason code.
    pub reason: RedactionReason,
}

/// The workspace a run is bound to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRef {
    /// Workspace root path.
    pub root: BoundedText,
    /// Branch the work happens on.
    pub branch: ExternalName,
    /// The commit the branch was cut from.
    pub baseline_commit: ExternalId,
}

/// The run and workspace identity captured when a pack is frozen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBinding {
    /// The run the pack was frozen for.
    pub agent_run_id: AgentRunId,
    /// The workspace it was frozen against.
    pub workspace: WorkspaceRef,
    /// When it was frozen.
    pub started_at: Timestamp,
}

/// A fully resolved Context Pack: the value, its canonical bytes, its digest,
/// its provenance and its redaction report.
///
/// Every field is owned. There is no loader, no source handle and no reference
/// back to the inputs, so a later change to a source cannot reach an already
/// resolved pack — the only way to observe the change is a new resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContextPack {
    realm_id: RealmId,
    resolved: serde_json::Value,
    document: CanonicalDocument,
    provenance: Vec<ProvenanceEntry>,
    redactions: Vec<RedactionRecord>,
}

impl ResolvedContextPack {
    /// Assemble a resolved pack. Crate-private: the resolver is the only way in.
    pub(crate) fn new(
        realm_id: RealmId,
        resolved: serde_json::Value,
        document: CanonicalDocument,
        provenance: Vec<ProvenanceEntry>,
        redactions: Vec<RedactionRecord>,
    ) -> Self {
        Self {
            realm_id,
            resolved,
            document,
            provenance,
            redactions,
        }
    }

    /// The realm the pack was resolved in.
    #[must_use]
    pub const fn realm_id(&self) -> RealmId {
        self.realm_id
    }

    /// The resolved pack value, after reference admission and redaction.
    #[must_use]
    pub const fn resolved(&self) -> &serde_json::Value {
        &self.resolved
    }

    /// The canonical document: pack, provenance and redaction report together.
    #[must_use]
    pub const fn document(&self) -> &CanonicalDocument {
        &self.document
    }

    /// The pack digest. This is the core document digest and nothing else.
    #[must_use]
    pub const fn hash(&self) -> &ContentHash {
        self.document.hash()
    }

    /// The envelope contract the pack was resolved under.
    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersion {
        self.document.schema_version()
    }

    /// The winning source of every resolved leaf, ordered by path.
    #[must_use]
    pub fn provenance(&self) -> &[ProvenanceEntry] {
        &self.provenance
    }

    /// Every applied redaction, ordered by path then source.
    #[must_use]
    pub fn redactions(&self) -> &[RedactionRecord] {
        &self.redactions
    }
}

/// A resolved pack frozen against one run.
///
/// Identical to the preview it came from except for the pack id and the run
/// binding: the digest, provenance and redaction report are the same values the
/// operator previewed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPackSnapshot {
    pack: ResolvedContextPack,
    context_pack_id: ContextPackId,
    run: RunBinding,
}

impl ContextPackSnapshot {
    /// Freeze a resolved pack against a run. Crate-private: `start_run` is the
    /// only way in.
    pub(crate) fn new(
        pack: ResolvedContextPack,
        context_pack_id: ContextPackId,
        run: RunBinding,
    ) -> Self {
        Self {
            pack,
            context_pack_id,
            run,
        }
    }

    /// This pack's identity.
    #[must_use]
    pub const fn context_pack_id(&self) -> ContextPackId {
        self.context_pack_id
    }

    /// The run and workspace the pack was frozen against.
    #[must_use]
    pub const fn run(&self) -> &RunBinding {
        &self.run
    }

    /// The frozen pack.
    #[must_use]
    pub const fn pack(&self) -> &ResolvedContextPack {
        &self.pack
    }

    /// The frozen pack's digest.
    #[must_use]
    pub const fn hash(&self) -> &ContentHash {
        self.pack.hash()
    }
}
