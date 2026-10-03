//! Durable public issuer-key metadata port (ASMA-8278).
//!
//! These operations persist public key identity and irreversible revocation;
//! they create no actor, permission, token, receipt or native effect. Returned
//! projections are detached metadata, not proof of current transaction-held
//! freshness, authentic acquisition, possession or admission authority.

use crate::id::{ContentHash, ExternalId, ProjectId, RealmId};

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
}
