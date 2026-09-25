//! Durable canonical-history proof progress. No method dispatches a message.

use kontor_core::id::{CanonicalDocument, ContentHash};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::repository::{backend, conflict};
use crate::{MessageIssuance, SqliteStore};

/// A previously confirmed occurrence in this exact binding and generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageProofAnchor {
    /// Existing message identity, never a newly minted challenge.
    pub message_id: String,
    /// The existing durable timeline numbering.
    pub epoch: u64,
    /// Exact position of the previously proved occurrence.
    pub sequence: u64,
}

/// A proof at one immutable page checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDeliveryProof {
    /// Stored evidence document schema.
    pub schema_version: u32,
    /// Exact original issuance.
    pub message_id: String,
    /// Digest of every immutable issuance identity field.
    pub issuance_hash: String,
    /// Original runtime generation.
    pub runtime_generation: u64,
    /// Existing evidence against which the transcript must agree.
    pub anchor: MessageProofAnchor,
    /// Tail observed when this proof began; never recaptured on retry.
    pub upper_sequence: u64,
    /// Canonical event digest at the frozen upper bound.
    pub upper_hash: String,
    /// Last continuously verified position from the epoch origin.
    pub through_sequence: u64,
    /// Matching occurrences seen so far.
    pub occurrences: u32,
    /// First matching position, if any.
    pub candidate_sequence: Option<u64>,
    /// Exact canonical digest of that candidate occurrence.
    pub candidate_hash: Option<String>,
    /// Digest of the canonical native user-message text, when the runtime supplies it.
    pub candidate_body_hash: Option<String>,
    /// The candidate event timestamp; replay never substitutes the current time.
    pub candidate_accepted_at: Option<kontor_core::id::Timestamp>,
    /// Whether the historical anchor has been read and matched exactly.
    pub anchor_seen: bool,
    /// `scanning`, `confirmed`, `absent` or `duplicate`; only confirmed pins delivery.
    pub state: String,
    /// Compare-and-swap revision.
    pub revision: u64,
}

/// Hash the recorded identity without including the subsequently discovered delivery.
pub fn message_issuance_digest(issued: &MessageIssuance) -> RepositoryResult<ContentHash> {
    Ok(CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1, "message_id": issued.message_id, "runtime_kind": issued.runtime_kind,
        "host": issued.host, "runtime_binding_id": issued.runtime_binding_id,
        "native_session_id": issued.native_session_id, "idempotency_key": issued.idempotency_key,
        "provenance": issued.provenance, "issued_at": issued.issued_at.to_string(),
        "boundary_at": issued.boundary_at,
    }))?
    .hash()
    .clone())
}

fn sql_number(value: u64) -> RepositoryResult<i64> {
    i64::try_from(value)
        .map_err(|_| conflict("message proof", "the position exceeds the durable range"))
}

fn document(proof: &MessageDeliveryProof) -> RepositoryResult<CanonicalDocument> {
    CanonicalDocument::from_value(&serde_json::json!(proof)).map_err(Into::into)
}

fn decode(json: String, hash: String) -> RepositoryResult<MessageDeliveryProof> {
    let value: serde_json::Value = serde_json::from_str(&json)
        .map_err(|_| conflict("message proof", "the stored proof is unreadable"))?;
    if CanonicalDocument::from_value(&value)?.hash().as_str() != hash {
        return Err(conflict(
            "message proof",
            "the stored proof digest disagrees",
        ));
    }
    serde_json::from_value(value)
        .map_err(|_| conflict("message proof", "the stored proof shape is invalid"))
}

impl SqliteStore {
    /// Read the current progress without touching the native runtime.
    pub fn message_delivery_proof(
        &self,
        message_id: &str,
    ) -> RepositoryResult<Option<MessageDeliveryProof>> {
        self.connection.query_row(
            "SELECT proof_json, proof_hash FROM runtime_message_delivery_proofs WHERE message_id = ?1",
            [message_id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(backend)?.map(|(json, hash)| decode(json, hash)).transpose()
    }

    /// Replay an exact page receipt, including after a daemon restart.
    pub fn message_delivery_proof_replay(
        &self,
        message_id: &str,
        key: &str,
        request_hash: &str,
    ) -> RepositoryResult<Option<MessageDeliveryProof>> {
        let held: Option<(String, String, String)> = self.connection.query_row(
            "SELECT request_hash, result_json, result_hash FROM runtime_message_delivery_proof_steps WHERE message_id = ?1 AND idempotency_key = ?2",
            params![message_id, key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional().map_err(backend)?;
        held.map(|(original, json, hash)| {
            if original != request_hash {
                return Err(conflict(
                    "message proof key",
                    "the key was already used for another page request",
                ));
            }
            decode(json, hash)
        })
        .transpose()
    }

    /// Find genuine earlier delivery/turn evidence in the same binding generation.
    pub fn message_delivery_proof_anchor(
        &self,
        issued: &MessageIssuance,
    ) -> RepositoryResult<Option<MessageProofAnchor>> {
        let held: Option<(String, i64, i64)> = self.connection.query_row(
            "SELECT message_id, epoch, sequence FROM (
                SELECT message_id, delivered_epoch AS epoch, delivered_sequence AS sequence, issued_at AS at
                  FROM runtime_message_issuances
                 WHERE runtime_binding_id = ?1 AND issued_at <= ?2 AND delivered_epoch IS NOT NULL
                UNION ALL
                SELECT t.runtime_message_id, t.message_timeline_epoch, t.message_timeline_sequence, t.settled_at
                  FROM role_turns t JOIN runtime_bindings b ON b.agent_run_id = t.agent_run_id
                 WHERE b.id = ?1 AND t.binding_generation = b.generation
                   AND t.settled_at <= ?2 AND t.runtime_message_id IS NOT NULL
                   AND t.message_timeline_epoch IS NOT NULL AND t.message_timeline_sequence IS NOT NULL
            ) ORDER BY at DESC LIMIT 1",
            params![issued.runtime_binding_id, issued.issued_at.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional().map_err(backend)?;
        held.map(|(message_id, epoch, sequence)| {
            Ok(MessageProofAnchor {
                message_id,
                epoch: u64::try_from(epoch)
                    .map_err(|_| conflict("message proof anchor", "negative epoch"))?,
                sequence: u64::try_from(sequence)
                    .map_err(|_| conflict("message proof anchor", "negative sequence"))?,
            })
        })
        .transpose()
    }

    /// Commit one strictly advancing proof page, its immutable replay result and
    /// a uniquely proved delivery atomically. Refusals leave all three untouched.
    pub fn advance_message_delivery_proof(
        &self,
        issued: &MessageIssuance,
        proof: &MessageDeliveryProof,
        expected_revision: u64,
        key: &str,
        request_hash: &str,
    ) -> RepositoryResult<MessageDeliveryProof> {
        let transaction = self.connection.unchecked_transaction().map_err(backend)?;
        if let Some(replayed) =
            self.message_delivery_proof_replay(&proof.message_id, key, request_hash)?
        {
            transaction.commit().map_err(backend)?;
            return Ok(replayed);
        }
        let current =
            self.message_issuance(&issued.message_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "message issuance",
                })?;
        if current != *issued
            || message_issuance_digest(&current)?.as_str() != proof.issuance_hash
            || proof.message_id != issued.message_id
        {
            return Err(conflict(
                "message proof",
                "the original issuance changed while history was read",
            ));
        }
        let exact_binding: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM runtime_bindings WHERE id = ?1 AND native_id = ?2 AND runtime_kind = ?3 AND generation = ?4)",
            params![issued.runtime_binding_id, issued.native_session_id, issued.runtime_kind, sql_number(proof.runtime_generation)?], |row| row.get(0),
        ).map_err(backend)?;
        if !exact_binding {
            return Err(conflict(
                "message proof",
                "the original native binding changed while history was read",
            ));
        }
        let previous = self.message_delivery_proof(&proof.message_id)?;
        if previous.as_ref().map_or(0, |held| held.revision) != expected_revision
            || Some(proof.revision) != expected_revision.checked_add(1)
        {
            return Err(conflict(
                "message proof revision",
                "re-read proof progress before advancing another page",
            ));
        }
        if proof.schema_version != 1
            || (previous.is_some() && proof.through_sequence == 0)
            || proof.through_sequence > proof.upper_sequence
            || proof.upper_sequence < proof.anchor.sequence
            || proof.anchor.epoch == 0
            || proof.anchor.sequence == 0
            || ContentHash::parse(&proof.issuance_hash).is_err()
            || ContentHash::parse(&proof.upper_hash).is_err()
            || proof
                .candidate_hash
                .as_ref()
                .is_some_and(|hash| ContentHash::parse(hash).is_err())
            || proof.occurrences > 2
            || (proof.occurrences == 0) != proof.candidate_sequence.is_none()
            || proof.candidate_sequence.is_some() != proof.candidate_hash.is_some()
            || proof.candidate_sequence.is_some() != proof.candidate_accepted_at.is_some()
            || (proof.candidate_sequence.is_none() && proof.candidate_body_hash.is_some())
            || proof
                .candidate_body_hash
                .as_ref()
                .is_some_and(|hash| ContentHash::parse(hash).is_err())
            || proof
                .candidate_sequence
                .is_some_and(|at| at == 0 || at > proof.through_sequence)
            || !matches!(
                proof.state.as_str(),
                "scanning" | "confirmed" | "absent" | "duplicate"
            )
        {
            return Err(conflict(
                "message proof",
                "the page has an impossible proof shape",
            ));
        }
        if let Some(held) = previous.as_ref() {
            if held.state != "scanning"
                || held.issuance_hash != proof.issuance_hash
                || held.anchor != proof.anchor
                || held.runtime_generation != proof.runtime_generation
                || held.upper_sequence != proof.upper_sequence
                || held.upper_hash != proof.upper_hash
                || proof.through_sequence <= held.through_sequence
                || proof.occurrences < held.occurrences
                || (held.anchor_seen && !proof.anchor_seen)
                || (held.candidate_sequence.is_some()
                    && (held.candidate_sequence != proof.candidate_sequence
                        || held.candidate_hash != proof.candidate_hash
                        || held.candidate_body_hash != proof.candidate_body_hash
                        || held.candidate_accepted_at != proof.candidate_accepted_at))
            {
                return Err(conflict(
                    "message proof",
                    "the page changed immutable identity or prior evidence",
                ));
            }
        } else if self.message_delivery_proof_anchor(issued)?.as_ref() != Some(&proof.anchor) {
            return Err(conflict(
                "message proof anchor",
                "the starting anchor is not genuine earlier evidence",
            ));
        }
        let complete = proof.through_sequence == proof.upper_sequence;
        let valid_state = match proof.state.as_str() {
            "scanning" => !complete && proof.occurrences < 2,
            "confirmed" => complete && proof.anchor_seen && proof.occurrences == 1,
            "absent" => complete && proof.anchor_seen && proof.occurrences == 0,
            "duplicate" => proof.occurrences == 2,
            _ => false,
        };
        if !valid_state {
            return Err(conflict(
                "message proof",
                "the verdict does not follow from the accumulated evidence",
            ));
        }
        let doc = document(proof)?;
        self.connection.execute(
            "INSERT INTO runtime_message_delivery_proofs(message_id, revision, proof_json, proof_hash) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(message_id) DO UPDATE SET revision = excluded.revision, proof_json = excluded.proof_json, proof_hash = excluded.proof_hash",
            params![proof.message_id, sql_number(proof.revision)?, doc.json(), doc.hash().as_str()],
        ).map_err(backend)?;
        if proof.state == "confirmed" {
            let position = proof.candidate_sequence.expect("validated candidate");
            if current
                .delivered_at
                .is_some_and(|held| held != (proof.anchor.epoch, position))
            {
                return Err(conflict(
                    "message delivery",
                    "the delivery was already recorded at another position",
                ));
            }
            self.connection.execute(
                "UPDATE runtime_message_issuances SET delivered_epoch = ?2, delivered_sequence = ?3 WHERE message_id = ?1",
                params![proof.message_id, sql_number(proof.anchor.epoch)?, sql_number(position)?],
            ).map_err(backend)?;
        }
        self.connection.execute(
            "INSERT INTO runtime_message_delivery_proof_steps(message_id, idempotency_key, request_hash, result_json, result_hash) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![proof.message_id, key, request_hash, doc.json(), doc.hash().as_str()],
        ).map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(proof.clone())
    }
}
