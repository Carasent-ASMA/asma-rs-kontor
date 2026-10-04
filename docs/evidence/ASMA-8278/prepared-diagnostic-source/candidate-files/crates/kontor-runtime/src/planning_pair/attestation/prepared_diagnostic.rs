//! Consistency of original signed bytes with supplied prepared public metadata.
//!
//! All projections and request facts are untrusted inputs. Success establishes
//! neither authentic acquisition, freshness, current seat provenance, possession,
//! custody nor native fencing. No transaction or effect is performed; no verified
//! object, actor, grant or permission is returned. Historical preparation metadata
//! is never reinterpreted as a current owner observation.
//!
//! Contract: attestation spec §9.8, refusal order and diagnostic-only boundary.
//! @see asma-modules/_docs/ai-orchestration/specs/spec-orchestration-seat-attestation.md:437

use kontor_core::id::ContentHash;
use kontor_core::repository::attestation_authority::{
    AttestationAuthorityProjection, AttestationTokenProjection, StoredAttestationKey,
    StoredPreparedAttestationToken,
};

use super::{
    APPLICATION_AUDIENCE, AttestationClaims, MAX_PAYLOAD_BYTES, MAX_PUBLIC_KEY_BYTES,
    PublicKeyEntry, PublicKeySnapshot, VerificationRequest, verify,
};

/// Fieldless first refusal; no credential or identity bytes are emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PreparedDiagnosticRefusal {
    /// Original payload or signature lies outside verifier size bounds.
    #[error("prepared diagnostic size refusal")]
    Size,
    /// Projection scopes or fixed request application differ.
    #[error("prepared diagnostic scope refusal")]
    Scope,
    /// No selected key was supplied.
    #[error("prepared diagnostic key missing")]
    KeyMissing,
    /// No selected prepared token was supplied.
    #[error("prepared diagnostic token missing")]
    TokenMissing,
    /// Revision ordering or bounded public metadata is invalid.
    #[error("prepared diagnostic projection refusal")]
    Projection,
    /// Selected key material, identity or registration commitment differs.
    #[error("prepared diagnostic key commitment refusal")]
    KeyCommitment,
    /// Prepared digest differs from the exact original payload bytes.
    #[error("prepared diagnostic token commitment refusal")]
    TokenCommitment,
    /// Supplied prepared token metadata records revocation.
    #[error("prepared diagnostic token revoked")]
    TokenRevoked,
    /// Prepared interval or signed metadata differs.
    #[error("prepared diagnostic metadata refusal")]
    Metadata,
    /// Existing verifier refused the original signature, claims or request.
    #[error("prepared diagnostic verification refusal")]
    Verification,
}

/// Diagnose supplied metadata and original signed bytes, granting no authority.
///
/// `Ok(())` means consistency only. This detached call does not establish a
/// trusted clock, current provenance, authentic acquisition, transaction-held
/// freshness or actual native possession/fencing. Later admission must acquire
/// and hold authentic owner facts and establish those capabilities separately.
///
/// Bounds are checked before payload parsing or DER cloning. The existing
/// verifier owns signature/strict-claims/request/time/lifetime checks. Historical
/// node, binding and run revisions remain unqualified preparation provenance.
///
/// # Errors
/// Returns the first fieldless refusal in attestation spec §9.8 order.
/// @see asma-modules/_docs/ai-orchestration/specs/spec-orchestration-seat-attestation.md:451
pub fn diagnose_prepared_attestation(
    payload: &[u8],
    signature: &[u8],
    key_projection: &AttestationAuthorityProjection,
    token_projection: &AttestationTokenProjection,
    request: &VerificationRequest<'_>,
) -> Result<(), PreparedDiagnosticRefusal> {
    use PreparedDiagnosticRefusal as Refusal;
    if payload.is_empty()
        || payload.len() > MAX_PAYLOAD_BYTES
        || !(256..=1_024).contains(&signature.len())
    {
        return Err(Refusal::Size);
    }
    if key_projection.scope != token_projection.scope
        || key_projection.scope.realm_id != request.scope.realm_id
        || key_projection.scope.project_id != request.scope.project_id
        || key_projection.scope.application.as_str() != APPLICATION_AUDIENCE
        || request.audience != APPLICATION_AUDIENCE
    {
        return Err(Refusal::Scope);
    }
    let key = key_projection
        .selected_key
        .as_ref()
        .ok_or(Refusal::KeyMissing)?;
    let token = token_projection
        .selected_token
        .as_ref()
        .ok_or(Refusal::TokenMissing)?;
    validate_projection(
        key_projection.head_revision,
        token_projection.head_revision,
        key,
        token,
    )?;
    let material_digest = ContentHash::of(&key.public_key_der);
    if key.material_digest != material_digest
        || token.key_material_digest != material_digest
        || key.issuer != token.issuer
        || key.key_id != token.key_id
        || key.registered_revision != token.key_registered_revision
    {
        return Err(Refusal::KeyCommitment);
    }
    if token.payload_digest != ContentHash::of(payload) {
        return Err(Refusal::TokenCommitment);
    }
    if token.revoked_revision.is_some() {
        return Err(Refusal::TokenRevoked);
    }
    if key.not_before >= key.expires_at
        || token.not_before >= token.expires_at
        || token.not_before < key.not_before
        || token.expires_at > key.expires_at
    {
        return Err(Refusal::Metadata);
    }
    let keys = PublicKeySnapshot {
        revision: key_projection.head_revision,
        keys: vec![PublicKeyEntry {
            issuer: key.issuer.clone(),
            key_id: key.key_id.clone(),
            public_key_der: key.public_key_der.clone(),
            not_before: key.not_before,
            expires_at: key.expires_at,
            revoked: key.revoked_revision.is_some(),
        }],
    };
    let verified = verify(payload, signature, &keys, request).map_err(|_| Refusal::Verification)?;
    if !matches_metadata(verified.claims(), token) {
        return Err(Refusal::Metadata);
    }
    Ok(())
}

fn validate_projection(
    key_head: u64,
    token_head: u64,
    key: &StoredAttestationKey,
    token: &StoredPreparedAttestationToken,
) -> Result<(), PreparedDiagnosticRefusal> {
    if !valid_registration(key.registered_revision, key_head)
        || !valid_registration(token.registered_revision, token_head)
        || !valid_registration(token.key_registered_revision, key_head)
        || token.preparation_key_head_revision < key.registered_revision
        || token.preparation_key_head_revision > key_head
        || !valid_revocation(key.revoked_revision, key.registered_revision, key_head)
        || !valid_revocation(
            token.revoked_revision,
            token.registered_revision,
            token_head,
        )
        || key.public_key_der.is_empty()
        || key.public_key_der.len() > MAX_PUBLIC_KEY_BYTES
        || [
            &key.issuer,
            &key.key_id,
            &token.issuer,
            &token.key_id,
            &token.token_id,
        ]
        .iter()
        .any(|id| id.as_str().len() > 256)
    {
        return Err(PreparedDiagnosticRefusal::Projection);
    }
    Ok(())
}

fn valid_registration(registered: u64, head: u64) -> bool {
    registered > 0 && registered <= head
}

fn valid_revocation(revoked: Option<u64>, registered: u64, head: u64) -> bool {
    revoked.is_none_or(|revision| registered < revision && revision <= head)
}

fn matches_metadata(claims: &AttestationClaims, token: &StoredPreparedAttestationToken) -> bool {
    claims.issuer == token.issuer
        && claims.key_id == token.key_id
        && claims.token_id == token.token_id
        && claims.scope.epic_id == token.mini_project_id
        && claims.scope.task_id == token.task_id
        && claims.seat_binding_id == token.seat_binding_id
        && claims.occupancy_generation == token.occupancy_generation
        && claims.native_identity.runtime_kind == token.native_identity.runtime_kind
        && claims.native_identity.host == token.native_identity.host
        && claims.native_identity.generation == token.native_identity.generation
        && claims.native_identity.native_id == token.native_identity.native_id
        && claims.not_before == token.not_before
        && claims.expires_at == token.expires_at
}

#[cfg(test)]
mod tests;
