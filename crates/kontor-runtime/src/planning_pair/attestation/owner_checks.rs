//! Pure comparison of verified claims with supplied owner observations.
//!
//! Success reports diagnostic consistency only. These facts are caller-supplied:
//! this module neither acquires nor authenticates them, proves their freshness,
//! holds a transaction, authenticates an actor, nor grants any permission. Actual
//! possession and atomic current-authority/replay/native-effect fencing remain
//! separate trusted-owner requirements. A matching historical observation is
//! not evidence that those requirements hold at admission or execution.
//!
//! Contract: living attestation spec §9.4 in
//! `asma-modules/_docs/ai-orchestration/specs/spec-orchestration-seat-attestation.md`.

use kontor_core::id::{ExternalId, SeatBindingId};

use super::{
    AttestationNativeIdentity, AttestationScope, PlanningPairOperation, PublicKeyEntry,
    VerifiedAttestation,
};

/// Supplied current token-ledger observation. Its authenticity and revision
/// must be acquired and held by the owner; these fields establish neither.
#[derive(Debug, Clone, Copy)]
pub struct TokenObservation<'a> {
    /// Exact observed token identity.
    pub token_id: &'a ExternalId,
    /// Whether this token is revoked in the supplied observation.
    pub revoked: bool,
    /// Inclusive observed validity start, in Unix seconds.
    pub not_before: u64,
    /// Exclusive observed validity end, in Unix seconds.
    pub expires_at: u64,
}

/// Supplied seat and topology observation. Matching a native identity proves
/// no live possession; no native/readiness or authority conversion is provided.
#[derive(Debug, Clone, Copy)]
pub struct SeatObservation<'a> {
    /// Whether the observed seat is active.
    pub active: bool,
    /// Whether the observed topology node is active.
    pub node_active: bool,
    /// Exact current realm, project, epic and optional task scope.
    pub scope: &'a AttestationScope,
    /// Current logical seat binding.
    pub seat_binding_id: SeatBindingId,
    /// Current nonzero occupancy generation in the owner's applicable domain.
    pub occupancy_generation: u64,
    /// Current exact native identity, including nonzero runtime generation.
    pub native_identity: &'a AttestationNativeIdentity,
}

/// Detached owner observations to compare, granting no authority. No field
/// claims that its acquisition was authentic or transactionally current.
#[derive(Debug, Clone, Copy)]
pub struct OwnerFacts<'a> {
    /// Supplied current key-set revision; must exactly match verification.
    pub key_set_revision: u64,
    /// Selected current public-key entry; identity must match the signed key.
    pub key: &'a PublicKeyEntry,
    /// Selected current token observation.
    pub token: TokenObservation<'a>,
    /// Current seat and node observation.
    pub seat: SeatObservation<'a>,
    /// Exact current requested operation, compared to the signed operation set.
    pub operation: PlanningPairOperation,
    /// Supplied current observation time, in Unix seconds.
    pub now: u64,
}

/// A fieldless diagnostic refusal, never an actor, grant or permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OwnerCheckRefusal {
    /// Current key-set revision is zero or differs from verification.
    #[error("owner key-set revision mismatch")]
    KeySetRevision,
    /// The selected issuer or key differs from the signed identity.
    #[error("owner key identity mismatch")]
    KeyIdentity,
    /// The selected token differs from the signed token identity.
    #[error("owner token identity mismatch")]
    TokenIdentity,
    /// The selected key is currently observed as revoked.
    #[error("owner key is revoked")]
    KeyRevoked,
    /// The selected token is currently observed as revoked.
    #[error("owner token is revoked")]
    TokenRevoked,
    /// Current key validity interval is invalid, future or expired.
    #[error("owner key validity interval refusal")]
    KeyValidity,
    /// Signed or current token validity interval is invalid, future or expired.
    #[error("owner token validity interval refusal")]
    TokenValidity,
    /// The observed seat is inactive.
    #[error("owner seat is inactive")]
    SeatInactive,
    /// The observed topology node is inactive.
    #[error("owner topology node is inactive")]
    NodeInactive,
    /// Current scope differs from the exact signed scope.
    #[error("owner scope mismatch")]
    Scope,
    /// Current binding differs from the exact signed seat binding.
    #[error("owner seat binding mismatch")]
    SeatBinding,
    /// Current occupancy generation is zero or differs from the signed one.
    #[error("owner occupancy generation mismatch")]
    OccupancyGeneration,
    /// Native generation is zero or the exact native identity differs.
    #[error("owner native identity mismatch")]
    NativeIdentity,
    /// The current operation is absent from the signed operation set.
    #[error("owner requested operation mismatch")]
    Operation,
}

/// Compare signed claims with supplied observations. `Ok(())` means only
/// diagnostic consistency; it is not admission, authentication or permission.
/// The owner must acquire authentic current observations, hold their revisions
/// atomically with receipt/state CAS and effect admission, and establish actual
/// native possession and execution-time fencing separately before any effect.
///
/// Both signed and observed token intervals must contain the observation time;
/// an observed interval cannot extend the signed token's expiry. Any key-set
/// revision change requires verification against the new snapshot first.
///
/// # Errors
/// Returns the first fieldless revision, identity, revocation, time or seat
/// comparison refusal. No supplied credential or identity bytes are emitted.
pub fn compare_owner_facts(
    verified: &VerifiedAttestation,
    facts: &OwnerFacts<'_>,
) -> Result<(), OwnerCheckRefusal> {
    let claims = verified.claims();
    if facts.key_set_revision == 0 || facts.key_set_revision != verified.snapshot_revision() {
        return Err(OwnerCheckRefusal::KeySetRevision);
    }
    if facts.key.issuer != claims.issuer || facts.key.key_id != claims.key_id {
        return Err(OwnerCheckRefusal::KeyIdentity);
    }
    if facts.token.token_id != &claims.token_id {
        return Err(OwnerCheckRefusal::TokenIdentity);
    }
    if facts.key.revoked {
        return Err(OwnerCheckRefusal::KeyRevoked);
    }
    if facts.token.revoked {
        return Err(OwnerCheckRefusal::TokenRevoked);
    }
    if !contains_time(facts.key.not_before, facts.key.expires_at, facts.now) {
        return Err(OwnerCheckRefusal::KeyValidity);
    }
    if !contains_time(claims.not_before, claims.expires_at, facts.now)
        || !contains_time(facts.token.not_before, facts.token.expires_at, facts.now)
    {
        return Err(OwnerCheckRefusal::TokenValidity);
    }
    if !facts.seat.active {
        return Err(OwnerCheckRefusal::SeatInactive);
    }
    if !facts.seat.node_active {
        return Err(OwnerCheckRefusal::NodeInactive);
    }
    if facts.seat.scope != &claims.scope {
        return Err(OwnerCheckRefusal::Scope);
    }
    if facts.seat.seat_binding_id != claims.seat_binding_id {
        return Err(OwnerCheckRefusal::SeatBinding);
    }
    if facts.seat.occupancy_generation == 0
        || facts.seat.occupancy_generation != claims.occupancy_generation
    {
        return Err(OwnerCheckRefusal::OccupancyGeneration);
    }
    if facts.seat.native_identity.generation == 0
        || facts.seat.native_identity != &claims.native_identity
    {
        return Err(OwnerCheckRefusal::NativeIdentity);
    }
    if !claims.allowed_operations.contains(&facts.operation) {
        return Err(OwnerCheckRefusal::Operation);
    }
    Ok(())
}

fn contains_time(not_before: u64, expires_at: u64, now: u64) -> bool {
    not_before < expires_at && not_before <= now && now < expires_at
}

#[cfg(test)]
mod tests;
