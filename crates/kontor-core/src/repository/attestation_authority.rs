//! Durable public issuer-key metadata port (ASMA-8278).
//!
//! These operations persist public key identity and irreversible revocation;
//! they create no actor, permission, issued token, receipt or native effect. Returned
//! projections are detached metadata, not proof of current transaction-held
//! freshness, authentic acquisition, possession or admission authority.

use crate::consultation::ConsultationRunId;
use crate::id::{
    AggregateRevision, ContentHash, ExternalId, MiniProjectId, ProjectId, RealmId, RoleSlotId,
    SeatBindingId, TaskId, TopologyNodeId,
};
use crate::state::NativeRuntimeIdentity;

use super::RepositoryResult;

/// Exact scope in the current store's Realm and one owning project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationAuthorityScope {
    /// Must name this store's immutable Realm, never a caller-selected file.
    pub realm_id: RealmId,
    /// The existing owning project.
    pub project_id: ProjectId,
    /// Exact application audience/key namespace, bounded at 256 UTF-8 bytes.
    pub application: ExternalId,
}

/// Register one immutable issuer/key mapping under an exact head expectation.
#[derive(Debug, Clone)]
pub struct RegisterAttestationKey {
    /// Existing Realm/project/application scope.
    pub scope: AttestationAuthorityScope,
    /// `None` explicitly expects no head; `Some` expects that positive revision.
    pub expected_head_revision: Option<u64>,
    /// Dedicated issuer identity, at most 256 UTF-8 bytes.
    pub issuer: ExternalId,
    /// Immutable key identity within this scope and issuer.
    pub key_id: ExternalId,
    /// Public DER bytes, nonempty and at most 2048; RSA validity is not asserted.
    pub public_key_der: Vec<u8>,
    /// Inclusive Unix-second validity start, representable by SQLite INTEGER.
    pub not_before: u64,
    /// Exclusive validity end, strictly later and representable by SQLite.
    pub expires_at: u64,
}

/// A retained immutable public-key mapping and its one-way revocation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAttestationKey {
    /// Immutable dedicated issuer identity.
    pub issuer: ExternalId,
    /// Immutable key identity.
    pub key_id: ExternalId,
    /// Exact registered public bytes; no private material.
    pub public_key_der: Vec<u8>,
    /// Digest of exactly the public DER bytes.
    pub material_digest: ContentHash,
    /// Inclusive registered validity start.
    pub not_before: u64,
    /// Exclusive registered validity end.
    pub expires_at: u64,
    /// Positive scope revision at registration.
    pub registered_revision: u64,
    /// Positive later scope revision at revocation, permanent once present.
    pub revoked_revision: Option<u64>,
}

/// One coherent head and selected key read. Nothing here is an authorizing
/// snapshot or proof that a subsequent transaction still has this revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationAuthorityProjection {
    /// Exact current-store scope.
    pub scope: AttestationAuthorityScope,
    /// Positive current key-set head revision at the read.
    pub head_revision: u64,
    /// Selected issuer/key, or `None` if absent within this existing scope.
    pub selected_key: Option<StoredAttestationKey>,
}

/// Expectations for a non-authorizing prepared payload commitment.
/// No signature, bearer bytes, key material or generation-domain selector is accepted.
#[derive(Debug, Clone)]
pub struct PrepareAttestationToken {
    /// Existing store Realm, project and exact application namespace.
    pub scope: AttestationAuthorityScope,
    /// Exact positive current key-set head.
    pub expected_key_head_revision: u64,
    /// `None` expects no token head; `Some` expects its exact positive revision.
    pub expected_token_head_revision: Option<u64>,
    /// Bounded issuer identity.
    pub issuer: ExternalId,
    /// Known immutable key identity.
    pub key_id: ExternalId,
    /// Permanent issuer-scoped identity, never reused across key rotations.
    pub token_id: ExternalId,
    /// Exact owning epic expected from stored membership.
    pub mini_project_id: MiniProjectId,
    /// Exact task scope, including absence; `None` is never a wildcard.
    pub task_id: Option<TaskId>,
    /// Actual logical seat whose stored provenance is resolved by the writer.
    pub seat_binding_id: SeatBindingId,
    /// Expected positive occupancy generation (not runtime generation).
    pub expected_occupancy_generation: u64,
    /// Expected exact current native metadata, not a possession proof.
    pub expected_native_identity: NativeRuntimeIdentity,
    /// Inclusive Unix-second start, contained in the selected key's interval.
    pub not_before: u64,
    /// Exclusive end, strictly later and contained in the key interval.
    pub expires_at: u64,
    /// Unqualified digest of intended payload bytes; authenticates nothing.
    pub payload_digest: ContentHash,
}

/// Stored-derived occupancy domain. Detached provenance is never authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttestationSeatProvenance {
    /// An actual hosted row, with generation derived from retained history + 1.
    Hosted,
    /// An actual consultation seat joined to its exact run, slot and node.
    Consultation {
        /// Family-qualified stored run identity.
        run_id: ConsultationRunId,
        /// Exact stored seat slot, also matching its binding.
        role_slot_id: RoleSlotId,
        /// Run revision observed inside preparation's writer transaction.
        run_revision: AggregateRevision,
    },
}

/// Permanent prepared metadata. This value can be freely constructed and
/// neither proves issuance/signature validity nor authorizes admission/effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPreparedAttestationToken {
    /// Immutable issuer identity.
    pub issuer: ExternalId,
    /// Immutable selected key identity.
    pub key_id: ExternalId,
    /// Permanent identity within project/application/issuer.
    pub token_id: ExternalId,
    /// Unqualified payload commitment.
    pub payload_digest: ContentHash,
    /// Digest of the selected immutable public material.
    pub key_material_digest: ContentHash,
    /// Selected key's registration revision, not today's key head.
    pub key_registered_revision: u64,
    /// Key-set head observed at preparation, retained only as provenance.
    pub preparation_key_head_revision: u64,
    /// Inclusive intended token validity start.
    pub not_before: u64,
    /// Exclusive intended token validity end.
    pub expires_at: u64,
    /// Exact stored owning epic.
    pub mini_project_id: MiniProjectId,
    /// Exact stored task, including absence.
    pub task_id: Option<TaskId>,
    /// Exact stored logical seat.
    pub seat_binding_id: SeatBindingId,
    /// Exact hosting node observed inside preparation.
    pub topology_node_id: TopologyNodeId,
    /// Binding revision observed inside preparation.
    pub binding_revision: AggregateRevision,
    /// Node revision observed inside preparation.
    pub node_revision: AggregateRevision,
    /// Actual stored-derived occupancy domain and run provenance.
    pub provenance: AttestationSeatProvenance,
    /// Positive current occupancy generation observed at preparation.
    pub occupancy_generation: u64,
    /// Exact native metadata observed at preparation; proves no possession.
    pub native_identity: NativeRuntimeIdentity,
    /// Positive separate token-ledger revision at preparation.
    pub registered_revision: u64,
    /// Permanent later token-ledger revocation revision, if present.
    pub revoked_revision: Option<u64>,
}

/// One coherent token head and selected permanent commitment. A detached
/// read is not authentic acquisition or transaction-held freshness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationTokenProjection {
    /// Exact store scope.
    pub scope: AttestationAuthorityScope,
    /// Positive current token-ledger head (independent of the key head).
    pub head_revision: u64,
    /// Selected issuer/token, or absence inside an existing token head.
    pub selected_token: Option<StoredPreparedAttestationToken>,
}

/// Public-key ledger operations behind the existing single state writer.
/// History is never capped at the verifier's 64-key transport bound.
pub trait AttestationAuthorityRepository {
    /// Register new material atomically with its next scope head revision.
    /// A previously registered key id can never be reused, even after revocation.
    ///
    /// # Errors
    /// Refuses unknown scope, invalid bounds, mismatched head, reused identity
    /// and checked revision/integer overflow without partial writes.
    fn register_attestation_key(
        &self,
        request: &RegisterAttestationKey,
    ) -> RepositoryResult<AttestationAuthorityProjection>;

    /// Irreversibly revoke one known key with the exact positive current head.
    /// Repeating an already revoked key at a matching head is an unchanged
    /// metadata readback; an old-head replay remains a conflict.
    ///
    /// # Errors
    /// Refuses absent scope/key, mismatched revision and checked overflow.
    fn revoke_attestation_key(
        &self,
        scope: &AttestationAuthorityScope,
        issuer: &ExternalId,
        key_id: &ExternalId,
        expected_head_revision: u64,
    ) -> RepositoryResult<AttestationAuthorityProjection>;

    /// Read one coherent head and selected issuer/key within the exact scope.
    /// A scope outside this Realm/project resolves to `None`, never another row.
    ///
    /// # Errors
    /// Refuses invalid transport bounds or malformed persisted metadata.
    fn read_attestation_key_authority(
        &self,
        scope: &AttestationAuthorityScope,
        issuer: &ExternalId,
        key_id: &ExternalId,
    ) -> RepositoryResult<Option<AttestationAuthorityProjection>>;
    /// Prepare metadata after fresh scope, heads, key and actual seat checks.
    /// Every identity reuse refuses, even with identical fields, after those
    /// checks. This is never replay, issuance or admission.
    ///
    /// # Errors
    /// Refuses invalid bounds, stale heads, revoked/absent key, invalid interval,
    /// ambiguous or mismatched stored membership/native, reuse and overflow.
    fn prepare_attestation_token(
        &self,
        request: &PrepareAttestationToken,
    ) -> RepositoryResult<AttestationTokenProjection>;

    /// Permanently revoke with the exact current token head. Expired tokens,
    /// revoked keys and retired seats remain revocable. A matching-head repeat
    /// is unchanged; a stale-head repeat conflicts.
    ///
    /// # Errors
    /// Refuses invalid bounds, unknown scope/token, stale head and overflow.
    fn revoke_prepared_attestation_token(
        &self,
        scope: &AttestationAuthorityScope,
        issuer: &ExternalId,
        token_id: &ExternalId,
        expected_token_head_revision: u64,
    ) -> RepositoryResult<AttestationTokenProjection>;

    /// Read coherent detached metadata within this Realm/project/application.
    ///
    /// # Errors
    /// Refuses invalid bounds and malformed persisted metadata.
    fn read_prepared_attestation_token(
        &self,
        scope: &AttestationAuthorityScope,
        issuer: &ExternalId,
        token_id: &ExternalId,
    ) -> RepositoryResult<Option<AttestationTokenProjection>>;
}
