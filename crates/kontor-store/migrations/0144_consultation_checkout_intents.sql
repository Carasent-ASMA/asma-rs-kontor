-- Placement is owned by its existing durable topology node, not a new registry.
-- Historical nodes remain unpinned; no backfill or inferred adoption.
ALTER TABLE topology_nodes ADD COLUMN consultation_intent TEXT NULL CHECK (consultation_intent IS NULL OR json_valid(consultation_intent));
ALTER TABLE topology_nodes ADD COLUMN consultation_intent_hash TEXT NULL;
ALTER TABLE topology_nodes ADD COLUMN consultation_git_readback TEXT NULL CHECK (consultation_git_readback IS NULL OR json_valid(consultation_git_readback));
CREATE TRIGGER consultation_intent_immutable BEFORE UPDATE ON topology_nodes
WHEN OLD.consultation_intent IS NOT NULL AND
 (NEW.consultation_intent IS NOT OLD.consultation_intent OR NEW.consultation_intent_hash IS NOT OLD.consultation_intent_hash)
BEGIN SELECT RAISE(ABORT, 'consultation.intent_immutable'); END;
CREATE TRIGGER consultation_readback_immutable BEFORE UPDATE ON topology_nodes
WHEN OLD.consultation_git_readback IS NOT NULL AND NEW.consultation_git_readback IS NOT OLD.consultation_git_readback
BEGIN SELECT RAISE(ABORT, 'consultation.readback_immutable'); END;
PRAGMA user_version = 144;
