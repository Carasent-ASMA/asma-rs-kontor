//! Bounded public-key snapshot transport, with no trust or authority claim.
//!
//! Decoding yields freely constructable, untrusted public configuration. JSON,
//! valid bounds or a revision establish neither authenticity, freshness nor
//! cryptographic DER validity. Trust acquisition belongs to the designated
//! owner; this codec never discovers keys, signs, authenticates or actuates.
//! It does not use or change `CanonicalDocument`'s credential scanner.
//!
//! Contract: living attestation spec §9.3 in
//! `asma-modules/_docs/ai-orchestration/specs/spec-orchestration-seat-attestation.md`.

use kontor_core::id::ExternalId;
use serde::{Deserialize, Serialize};

use super::{PublicKeyEntry, PublicKeySnapshot};

/// Maximum input and output size, checked separately from structural bounds.
pub const MAX_SNAPSHOT_BYTES: usize = 1_048_576;
const MAX_IDENTIFIER_BYTES: usize = 256;

/// A fieldless transport refusal; no supplied identity or credential bytes
/// are retained or printed, and no variant establishes trusted authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotCodecRefusal {
    /// Input or serialized output exceeds the one-MiB ceiling.
    #[error("public snapshot exceeds transport size bound")]
    Size,
    /// Malformed JSON, unknown/duplicate fields or invalid typed identifiers.
    #[error("invalid public snapshot payload")]
    Payload,
    /// The exact version-one schema is not selected.
    #[error("unsupported public snapshot schema")]
    Schema,
    /// Shared revision, count, DER, interval or pair-uniqueness guards refuse.
    #[error("invalid public snapshot structure")]
    Snapshot,
    /// An issuer or key identifier exceeds the transport UTF-8 byte limit.
    #[error("public snapshot identifier exceeds byte bound")]
    IdentifierBytes,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSnapshot {
    schema_version: u16,
    revision: u64,
    keys: Vec<WireKey>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireKey {
    issuer: ExternalId,
    key_id: ExternalId,
    public_key_der: Vec<u8>,
    not_before: u64,
    expires_at: u64,
    revoked: bool,
}

/// Decode bounded version-one JSON into untrusted public configuration.
/// No key source, freshness, DER validity or admission is established.
///
/// # Errors
/// Refuses oversize input before parsing, then malformed/unsupported schemas
/// and invalid typed or structural snapshot fields.
pub fn decode(input: &[u8]) -> Result<PublicKeySnapshot, SnapshotCodecRefusal> {
    if input.len() > MAX_SNAPSHOT_BYTES {
        return Err(SnapshotCodecRefusal::Size);
    }
    let wire: WireSnapshot =
        serde_json::from_slice(input).map_err(|_| SnapshotCodecRefusal::Payload)?;
    if wire.schema_version != 1 {
        return Err(SnapshotCodecRefusal::Schema);
    }
    let snapshot = PublicKeySnapshot {
        revision: wire.revision,
        keys: wire
            .keys
            .into_iter()
            .map(|key| PublicKeyEntry {
                issuer: key.issuer,
                key_id: key.key_id,
                public_key_der: key.public_key_der,
                not_before: key.not_before,
                expires_at: key.expires_at,
                revoked: key.revoked,
            })
            .collect(),
    };
    validate_transport(&snapshot)?;
    Ok(snapshot)
}

/// Encode validated structural public configuration as version-one JSON.
/// Validation precedes cloning; output is bounded and carries no trust claim.
///
/// # Errors
/// Refuses invalid snapshots before cloning and oversized serialized output.
pub fn encode(snapshot: &PublicKeySnapshot) -> Result<Vec<u8>, SnapshotCodecRefusal> {
    validate_transport(snapshot)?;
    let wire = WireSnapshot {
        schema_version: 1,
        revision: snapshot.revision,
        keys: snapshot
            .keys
            .iter()
            .map(|key| WireKey {
                issuer: key.issuer.clone(),
                key_id: key.key_id.clone(),
                public_key_der: key.public_key_der.clone(),
                not_before: key.not_before,
                expires_at: key.expires_at,
                revoked: key.revoked,
            })
            .collect(),
    };
    let output = serde_json::to_vec(&wire).map_err(|_| SnapshotCodecRefusal::Payload)?;
    if output.len() > MAX_SNAPSHOT_BYTES {
        return Err(SnapshotCodecRefusal::Size);
    }
    Ok(output)
}

fn validate_transport(snapshot: &PublicKeySnapshot) -> Result<(), SnapshotCodecRefusal> {
    super::validate_snapshot(snapshot).map_err(|_| SnapshotCodecRefusal::Snapshot)?;
    if snapshot.keys.iter().any(|key| {
        key.issuer.as_str().len() > MAX_IDENTIFIER_BYTES
            || key.key_id.as_str().len() > MAX_IDENTIFIER_BYTES
    }) {
        return Err(SnapshotCodecRefusal::IdentifierBytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
