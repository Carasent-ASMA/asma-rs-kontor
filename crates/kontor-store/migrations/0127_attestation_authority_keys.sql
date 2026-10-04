-- ASMA-8278: public issuer-key metadata only. No issuance or native admission.
CREATE TABLE attestation_authority_heads (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    application TEXT NOT NULL CHECK (length(CAST(application AS BLOB)) BETWEEN 1 AND 256),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    PRIMARY KEY (project_id, application)
) STRICT;

CREATE TABLE attestation_authority_keys (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    application TEXT NOT NULL CHECK (length(CAST(application AS BLOB)) BETWEEN 1 AND 256),
    issuer TEXT NOT NULL CHECK (length(CAST(issuer AS BLOB)) BETWEEN 1 AND 256),
    key_id TEXT NOT NULL CHECK (length(CAST(key_id AS BLOB)) BETWEEN 1 AND 256),
    public_key_der BLOB NOT NULL CHECK (length(public_key_der) BETWEEN 1 AND 2048),
    material_digest TEXT NOT NULL CHECK (length(material_digest) = 64
                                        AND material_digest NOT GLOB '*[^0-9a-f]*'),
    not_before INTEGER NOT NULL CHECK (not_before >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > not_before),
    registered_revision INTEGER NOT NULL CHECK (registered_revision >= 1),
    revoked_revision INTEGER NULL CHECK (revoked_revision IS NULL
                                         OR revoked_revision > registered_revision),
    PRIMARY KEY (project_id, application, issuer, key_id),
    UNIQUE (project_id, application, registered_revision),
    UNIQUE (project_id, application, revoked_revision)
) STRICT;

CREATE TRIGGER attestation_keys_registration_next
BEFORE INSERT ON attestation_authority_keys
WHEN EXISTS (SELECT 1 FROM attestation_authority_keys
             WHERE project_id = NEW.project_id AND application = NEW.application
               AND issuer = NEW.issuer AND key_id = NEW.key_id)
  OR NEW.revoked_revision IS NOT NULL OR NEW.registered_revision <>
    COALESCE((SELECT revision FROM attestation_authority_heads
              WHERE project_id = NEW.project_id AND application = NEW.application), 0) + 1
BEGIN SELECT RAISE(ABORT, 'key registration must advance the current head exactly once'); END;

CREATE TRIGGER attestation_heads_initial
BEFORE INSERT ON attestation_authority_heads
WHEN EXISTS (SELECT 1 FROM attestation_authority_heads
             WHERE project_id = NEW.project_id AND application = NEW.application)
  OR NEW.revision <> 1 OR NOT EXISTS (
    SELECT 1 FROM attestation_authority_keys WHERE project_id = NEW.project_id
      AND application = NEW.application AND registered_revision = 1)
BEGIN SELECT RAISE(ABORT, 'an initial head requires its first registered key'); END;

CREATE TRIGGER attestation_heads_monotonic
BEFORE UPDATE ON attestation_authority_heads
WHEN NEW.project_id <> OLD.project_id OR NEW.application <> OLD.application
  OR NEW.revision <> OLD.revision + 1 OR NOT EXISTS (
    SELECT 1 FROM attestation_authority_keys WHERE project_id = NEW.project_id
      AND application = NEW.application AND
        (registered_revision = NEW.revision OR revoked_revision = NEW.revision))
BEGIN SELECT RAISE(ABORT, 'a head advances only with its exact key mutation'); END;

CREATE TRIGGER attestation_heads_permanent BEFORE DELETE ON attestation_authority_heads
BEGIN SELECT RAISE(ABORT, 'key authority heads are permanent'); END;

CREATE TRIGGER attestation_keys_immutable
BEFORE UPDATE ON attestation_authority_keys
WHEN NEW.project_id <> OLD.project_id OR NEW.application <> OLD.application
  OR NEW.issuer <> OLD.issuer OR NEW.key_id <> OLD.key_id
  OR NEW.public_key_der <> OLD.public_key_der OR NEW.material_digest <> OLD.material_digest
  OR NEW.not_before <> OLD.not_before OR NEW.expires_at <> OLD.expires_at
  OR NEW.registered_revision <> OLD.registered_revision
  OR OLD.revoked_revision IS NOT NULL OR NEW.revoked_revision IS NULL
  OR NEW.revoked_revision <> (SELECT revision + 1 FROM attestation_authority_heads
      WHERE project_id = OLD.project_id AND application = OLD.application)
BEGIN SELECT RAISE(ABORT, 'key mapping is immutable and revocation is one way'); END;

CREATE TRIGGER attestation_keys_permanent BEFORE DELETE ON attestation_authority_keys
BEGIN SELECT RAISE(ABORT, 'registered public key material is permanent'); END;

CREATE TRIGGER attestation_key_registration_head AFTER INSERT ON attestation_authority_keys
BEGIN
    INSERT INTO attestation_authority_heads (project_id, application, revision)
        SELECT NEW.project_id, NEW.application, 1 WHERE NOT EXISTS (
            SELECT 1 FROM attestation_authority_heads WHERE project_id = NEW.project_id
                AND application = NEW.application);
    UPDATE attestation_authority_heads SET revision = NEW.registered_revision
        WHERE project_id = NEW.project_id AND application = NEW.application
            AND revision = NEW.registered_revision - 1;
END;

CREATE TRIGGER attestation_key_revocation_head AFTER UPDATE ON attestation_authority_keys
BEGIN
    UPDATE attestation_authority_heads SET revision = NEW.revoked_revision
        WHERE project_id = NEW.project_id AND application = NEW.application
            AND revision = NEW.revoked_revision - 1;
END;

PRAGMA user_version = 127;
