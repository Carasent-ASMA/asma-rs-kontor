//! Verification-only seat attestations (ASMA-8278 B2).
//!
//! This seam verifies signed claims against an owner-supplied public-key
//! snapshot. It does not authenticate a live caller, establish possession,
//! grant authority, change readiness or permit native effects. The trusted
//! owner must still check current generation, fresh revocation, active seat
//! and node, native-session possession and effect fencing before replay and
//! atomically at admission. Snapshot freshness is not established here.
//!
//! No key is discovered from a token or URL. No signing or key-custody path
//! is provided. Signatures cover the domain prefix and exact payload bytes,
//! without JSON reserialization or the credential-scanning canonicalizer.
//!
//! Requirements: REQ-001–003, SEC-001–004 and CON-001–003 in
//! `asma-modules/_docs/ai-orchestration/specs/spec-orchestration-seat-attestation.md:49`.

use std::collections::BTreeSet;

use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
use kontor_core::id::{
    ExternalId, ExternalName, MiniProjectId, ProjectId, RealmId, RuntimeKindKey, SeatBindingId,
    TaskId,
};
use kontor_core::state::NativeRuntimeIdentity;
use serde::{Deserialize, Serialize};

pub mod owner_checks;
pub mod snapshot_codec;

/// The sole application audience accepted by this verifier.
pub const APPLICATION_AUDIENCE: &str = "asma.planning-pair.application.v1";
/// Maximum serialized attestation payload, checked before parsing.
pub const MAX_PAYLOAD_BYTES: usize = 8_192;
/// Domain separation for the fixed RSA PKCS#1 SHA-256 signature.
const SIGNATURE_DOMAIN: &[u8] = b"asma-seat-attestation-v1\0";
const MAX_KEYS: usize = 64;
const MAX_PUBLIC_KEY_BYTES: usize = 2_048;

/// The six planning-pair operations a token may name. Profile application
/// and every other command family are deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningPairOperation {
    /// Invoke one pair under an already selected profile.
    InvokePlanningPairRun,
    /// Record one member's finding.
    RecordPlanningPairFinding,
    /// Request the caller's clarification.
    RequestPlanningPairClarification,
    /// Record one member's clarification answer.
    RecordPlanningPairAnswer,
    /// Record the caller's disposition.
    RecordPlanningPairDisposition,
    /// Requalify one existing member session.
    RecoverPlanningPairSeat,
}

/// Exact scope carried by the claims and expected by the owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestationScope {
    /// Realm isolation boundary.
    pub realm_id: RealmId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Owning epic.
    pub epic_id: MiniProjectId,
    /// Exact task when the token is task-scoped.
    pub task_id: Option<TaskId>,
}

/// Strict wire form of a native identity. Matching it proves only that the
/// signer named the expected identity, never live possession of that session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestationNativeIdentity {
    /// Native runtime family.
    pub runtime_kind: RuntimeKindKey,
    /// Host or endpoint that owns the runtime generation.
    pub host: ExternalName,
    /// Nonzero native runtime generation.
    pub generation: u64,
    /// Exact session identifier within that runtime generation.
    pub native_id: ExternalId,
}

impl From<&NativeRuntimeIdentity> for AttestationNativeIdentity {
    fn from(identity: &NativeRuntimeIdentity) -> Self {
        Self {
            runtime_kind: identity.runtime_kind.clone(),
            host: identity.host.clone(),
            generation: identity.generation,
            native_id: identity.native_id.clone(),
        }
    }
}

/// Version-one claims. Constructing or deserializing these claims verifies
/// nothing; only [`verify`] produces [`VerifiedAttestation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestationClaims {
    /// Must be exactly one.
    pub schema_version: u16,
    /// Dedicated issuer identifier, selected from the public-key snapshot.
    pub issuer: ExternalId,
    /// Exact key revision selected from that issuer's snapshot entries.
    pub key_id: ExternalId,
    /// Must equal [`APPLICATION_AUDIENCE`].
    pub audience: String,
    /// Exact realm, project, epic and optional task scope.
    pub scope: AttestationScope,
    /// Exact logical seat.
    pub seat_binding_id: SeatBindingId,
    /// Nonzero occupancy generation asserted by the issuer.
    pub occupancy_generation: u64,
    /// Exact native session identity asserted by the issuer.
    pub native_identity: AttestationNativeIdentity,
    /// Nonempty, unique permitted operation names.
    pub allowed_operations: Vec<PlanningPairOperation>,
    /// Issuer token identifier for later owner revocation/replay checks.
    pub token_id: ExternalId,
    /// Inclusive validity start, in Unix seconds.
    pub not_before: u64,
    /// Exclusive validity end, in Unix seconds.
    pub expires_at: u64,
}

/// One trusted public-key entry supplied by the owner, never by the token.
#[derive(Debug, Clone)]
pub struct PublicKeyEntry {
    /// Dedicated issuer identifier.
    pub issuer: ExternalId,
    /// Key revision identifier within that issuer.
    pub key_id: ExternalId,
    /// RSA public key in DER form supported by the fixed verifier.
    pub public_key_der: Vec<u8>,
    /// Inclusive key validity start, in Unix seconds.
    pub not_before: u64,
    /// Exclusive key validity end, in Unix seconds.
    pub expires_at: u64,
    /// Revocation state in this snapshot, not a freshness assertion.
    pub revoked: bool,
}

/// An owner-supplied versioned public-key snapshot. Its source, authenticity
/// and revocation freshness must be established by the trusted owner.
#[derive(Debug, Clone)]
pub struct PublicKeySnapshot {
    /// Nonzero owner revision, retained in the verification result.
    pub revision: u64,
    /// Known issuer/key pairs; duplicate pairs are refused.
    pub keys: Vec<PublicKeyEntry>,
}

/// Explicit verification policy, without a permissive default.
#[derive(Debug, Clone, Copy)]
pub struct VerificationPolicy {
    /// Maximum allowed token lifetime in seconds; must be nonzero.
    pub maximum_lifetime_seconds: u64,
}

/// Expected request facts provided by the trusted owner.
#[derive(Debug)]
pub struct VerificationRequest<'a> {
    /// Explicit expected audience; must be the fixed application audience.
    pub audience: &'a str,
    /// Exact scope of this request.
    pub scope: &'a AttestationScope,
    /// Expected seat from owner state.
    pub seat_binding_id: SeatBindingId,
    /// Expected nonzero occupancy generation from owner state.
    pub occupancy_generation: u64,
    /// Expected native identity from owner state; no possession proof.
    pub native_identity: &'a AttestationNativeIdentity,
    /// Exact requested operation.
    pub operation: PlanningPairOperation,
    /// Trusted current time, in Unix seconds.
    pub now: u64,
    /// Explicit token lifetime bound.
    pub policy: VerificationPolicy,
}

/// Verified signed claims, granting no authority. Its private fields and lack
/// of a public constructor or deserializer prevent unchecked construction.
///
/// ```compile_fail
/// use kontor_runtime::planning_pair::attestation::{AttestationClaims, VerifiedAttestation};
/// fn forge(claims: AttestationClaims) -> VerifiedAttestation {
///     VerifiedAttestation { claims, snapshot_revision: 1 }
/// }
/// ```
///
/// ```compile_fail
/// use kontor_runtime::planning_pair::attestation::VerifiedAttestation;
/// let unchecked = serde_json::from_str::<VerifiedAttestation>("{}");
/// ```
#[derive(Debug)]
pub struct VerifiedAttestation {
    claims: AttestationClaims,
    snapshot_revision: u64,
}

impl VerifiedAttestation {
    /// Borrow the signed claims for subsequent trusted owner checks.
    #[must_use]
    pub const fn claims(&self) -> &AttestationClaims {
        &self.claims
    }

    /// Snapshot revision used here; the owner must still establish freshness.
    #[must_use]
    pub const fn snapshot_revision(&self) -> u64 {
        self.snapshot_revision
    }
}

/// A verification refusal. No variant establishes caller or effect authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AttestationRefusal {
    /// Payload or signature exceeds the fixed pre-parse bounds.
    #[error("attestation size outside fixed bounds")]
    Size,
    /// Unknown or duplicate fields, malformed JSON or invalid typed ids.
    #[error("invalid attestation payload")]
    Payload,
    /// Unsupported version, zero generation, repeated operations or interval.
    #[error("invalid attestation claims")]
    Claims,
    /// Invalid explicit policy or expected request generation.
    #[error("invalid verification request")]
    Request,
    /// Invalid revision, key bounds, intervals or duplicate issuer/key pairs.
    #[error("invalid public-key snapshot")]
    Snapshot,
    /// Issuer/key pair is absent from the supplied snapshot.
    #[error("unknown attestation issuer or key")]
    UnknownKey,
    /// Snapshot marks the selected key as revoked.
    #[error("attestation key is revoked")]
    RevokedKey,
    /// Key is not current, or the token extends outside its validity interval.
    #[error("attestation key is outside its validity interval")]
    KeyValidity,
    /// Exact payload bytes do not verify under the selected RSA public key.
    #[error("attestation signature does not verify")]
    Signature,
    /// Signed audience is not the fixed application audience.
    #[error("attestation audience mismatch")]
    Audience,
    /// Signed scope does not exactly match the owner request.
    #[error("attestation scope mismatch")]
    Scope,
    /// Signed seat or occupancy generation differs from the request.
    #[error("attestation seat or generation mismatch")]
    Seat,
    /// Signed native identity differs from the owner request.
    #[error("attestation native identity mismatch")]
    NativeIdentity,
    /// Requested operation is not among the signed permitted operations.
    #[error("attestation operation mismatch")]
    Operation,
    /// Token is future, expired or exceeds the explicit lifetime policy.
    #[error("attestation outside permitted validity interval")]
    Validity,
}

/// Verify the exact payload and detached signature using the fixed algorithm.
/// A successful result is input to later owner checks, never admission.
///
/// # Errors
/// Returns the first structural, cryptographic or exact request refusal.
pub fn verify(
    payload: &[u8],
    signature: &[u8],
    keys: &PublicKeySnapshot,
    request: &VerificationRequest<'_>,
) -> Result<VerifiedAttestation, AttestationRefusal> {
    if payload.is_empty()
        || payload.len() > MAX_PAYLOAD_BYTES
        || !(256..=1_024).contains(&signature.len())
    {
        return Err(AttestationRefusal::Size);
    }
    if request.audience != APPLICATION_AUDIENCE
        || request.policy.maximum_lifetime_seconds == 0
        || request.occupancy_generation == 0
        || request.native_identity.generation == 0
    {
        return Err(AttestationRefusal::Request);
    }
    let claims: AttestationClaims =
        serde_json::from_slice(payload).map_err(|_| AttestationRefusal::Payload)?;
    let lifetime = validate_claims(&claims)?;
    let key = select_key(keys, &claims)?;
    if key.revoked {
        return Err(AttestationRefusal::RevokedKey);
    }
    if request.now < key.not_before
        || request.now >= key.expires_at
        || claims.not_before < key.not_before
        || claims.expires_at > key.expires_at
    {
        return Err(AttestationRefusal::KeyValidity);
    }
    let mut signed = Vec::with_capacity(SIGNATURE_DOMAIN.len() + payload.len());
    signed.extend_from_slice(SIGNATURE_DOMAIN);
    signed.extend_from_slice(payload);
    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &key.public_key_der)
        .verify(&signed, signature)
        .map_err(|_| AttestationRefusal::Signature)?;
    if claims.audience != APPLICATION_AUDIENCE {
        return Err(AttestationRefusal::Audience);
    }
    if &claims.scope != request.scope {
        return Err(AttestationRefusal::Scope);
    }
    if claims.seat_binding_id != request.seat_binding_id
        || claims.occupancy_generation != request.occupancy_generation
    {
        return Err(AttestationRefusal::Seat);
    }
    if &claims.native_identity != request.native_identity {
        return Err(AttestationRefusal::NativeIdentity);
    }
    if !claims.allowed_operations.contains(&request.operation) {
        return Err(AttestationRefusal::Operation);
    }
    if request.now < claims.not_before
        || request.now >= claims.expires_at
        || lifetime > request.policy.maximum_lifetime_seconds
    {
        return Err(AttestationRefusal::Validity);
    }
    Ok(VerifiedAttestation {
        claims,
        snapshot_revision: keys.revision,
    })
}

fn validate_claims(claims: &AttestationClaims) -> Result<u64, AttestationRefusal> {
    if claims.schema_version != 1
        || claims.occupancy_generation == 0
        || claims.native_identity.generation == 0
        || claims.allowed_operations.is_empty()
        || claims.allowed_operations.len() > 6
        || claims
            .allowed_operations
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != claims.allowed_operations.len()
    {
        return Err(AttestationRefusal::Claims);
    }
    claims
        .expires_at
        .checked_sub(claims.not_before)
        .filter(|lifetime| *lifetime > 0)
        .ok_or(AttestationRefusal::Claims)
}

fn select_key<'a>(
    keys: &'a PublicKeySnapshot,
    claims: &AttestationClaims,
) -> Result<&'a PublicKeyEntry, AttestationRefusal> {
    validate_snapshot(keys)?;
    keys.keys
        .iter()
        .find(|key| key.issuer == claims.issuer && key.key_id == claims.key_id)
        .ok_or(AttestationRefusal::UnknownKey)
}

fn validate_snapshot(keys: &PublicKeySnapshot) -> Result<(), AttestationRefusal> {
    if keys.revision == 0 || keys.keys.is_empty() || keys.keys.len() > MAX_KEYS {
        return Err(AttestationRefusal::Snapshot);
    }
    let mut identities = BTreeSet::new();
    for key in &keys.keys {
        if key.public_key_der.is_empty()
            || key.public_key_der.len() > MAX_PUBLIC_KEY_BYTES
            || key.not_before >= key.expires_at
            || !identities.insert((key.issuer.as_str(), key.key_id.as_str()))
        {
            return Err(AttestationRefusal::Snapshot);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
