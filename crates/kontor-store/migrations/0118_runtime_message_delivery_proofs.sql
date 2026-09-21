-- Bounded, resumable evidence for legacy sends with no pre-send boundary.
-- No native dispatch is authorized by either table. The historical issuance,
-- observed upper bound, epoch and anchor remain immutable; each page is CAS.
CREATE TABLE runtime_message_delivery_proofs (
    message_id TEXT PRIMARY KEY REFERENCES runtime_message_issuances(message_id),
    revision INTEGER NOT NULL CHECK (revision > 0),
    proof_json TEXT NOT NULL CHECK (json_valid(proof_json)),
    proof_hash TEXT NOT NULL CHECK (length(proof_hash) = 64),
    CHECK (json_type(proof_json) IS 'object'),
    CHECK (json_extract(proof_json, '$.schema_version') IS 1),
    CHECK (json_extract(proof_json, '$.message_id') IS message_id),
    CHECK (json_extract(proof_json, '$.revision') IS revision)
) STRICT;
CREATE TRIGGER message_delivery_proof_forward_only BEFORE UPDATE ON runtime_message_delivery_proofs
WHEN NEW.message_id != OLD.message_id OR NEW.revision != OLD.revision + 1
  OR json_extract(OLD.proof_json, '$.state') != 'scanning'
  OR json_extract(NEW.proof_json, '$.issuance_hash') != json_extract(OLD.proof_json, '$.issuance_hash')
  OR json_extract(NEW.proof_json, '$.runtime_generation') != json_extract(OLD.proof_json, '$.runtime_generation')
  OR json_extract(NEW.proof_json, '$.upper_sequence') != json_extract(OLD.proof_json, '$.upper_sequence')
  OR json_extract(NEW.proof_json, '$.upper_hash') != json_extract(OLD.proof_json, '$.upper_hash')
  OR json_extract(NEW.proof_json, '$.anchor') != json_extract(OLD.proof_json, '$.anchor')
  OR json_extract(NEW.proof_json, '$.through_sequence') <= json_extract(OLD.proof_json, '$.through_sequence')
  OR json_extract(NEW.proof_json, '$.occurrences') < json_extract(OLD.proof_json, '$.occurrences')
  OR (json_extract(OLD.proof_json, '$.anchor_seen') = 1 AND json_extract(NEW.proof_json, '$.anchor_seen') IS NOT 1)
  OR (json_extract(OLD.proof_json, '$.candidate_sequence') IS NOT NULL AND (
       json_extract(NEW.proof_json, '$.candidate_sequence') IS NOT json_extract(OLD.proof_json, '$.candidate_sequence')
    OR json_extract(NEW.proof_json, '$.candidate_hash') IS NOT json_extract(OLD.proof_json, '$.candidate_hash')
    OR json_extract(NEW.proof_json, '$.candidate_body_hash') IS NOT json_extract(OLD.proof_json, '$.candidate_body_hash')
    OR json_extract(NEW.proof_json, '$.candidate_accepted_at') IS NOT json_extract(OLD.proof_json, '$.candidate_accepted_at')
  ))
BEGIN SELECT RAISE(ABORT, 'delivery proof identity and progress are immutable'); END;
CREATE TRIGGER message_delivery_proof_no_delete BEFORE DELETE ON runtime_message_delivery_proofs
BEGIN SELECT RAISE(ABORT, 'delivery proof history is retained'); END;
CREATE TABLE runtime_message_delivery_proof_steps (
    message_id TEXT NOT NULL REFERENCES runtime_message_delivery_proofs(message_id),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    request_hash TEXT NOT NULL CHECK (length(request_hash) = 64),
    result_json TEXT NOT NULL CHECK (json_valid(result_json)),
    result_hash TEXT NOT NULL CHECK (length(result_hash) = 64),
    PRIMARY KEY (message_id, idempotency_key)
) STRICT;
CREATE TRIGGER message_delivery_proof_steps_no_update BEFORE UPDATE ON runtime_message_delivery_proof_steps
BEGIN SELECT RAISE(ABORT, 'delivery proof receipts are immutable'); END;
CREATE TRIGGER message_delivery_proof_steps_no_delete BEFORE DELETE ON runtime_message_delivery_proof_steps
BEGIN SELECT RAISE(ABORT, 'delivery proof receipts are immutable'); END;
PRAGMA user_version = 118;
