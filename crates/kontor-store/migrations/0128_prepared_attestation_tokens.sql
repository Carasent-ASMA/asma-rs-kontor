-- ASMA-8278: permanent prepared commitments only; never issuance/admission.
CREATE TABLE attestation_token_heads (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    application TEXT NOT NULL CHECK (length(CAST(application AS BLOB)) BETWEEN 1 AND 256),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    PRIMARY KEY (project_id, application)
) STRICT;

CREATE TABLE prepared_attestation_tokens (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    application TEXT NOT NULL CHECK (length(CAST(application AS BLOB)) BETWEEN 1 AND 256),
    issuer TEXT NOT NULL CHECK (length(CAST(issuer AS BLOB)) BETWEEN 1 AND 256),
    key_id TEXT NOT NULL CHECK (length(CAST(key_id AS BLOB)) BETWEEN 1 AND 256),
    token_id TEXT NOT NULL CHECK (length(CAST(token_id AS BLOB)) BETWEEN 1 AND 256),
    payload_digest TEXT NOT NULL CHECK (length(payload_digest)=64 AND payload_digest NOT GLOB '*[^0-9a-f]*'),
    key_material_digest TEXT NOT NULL CHECK (length(key_material_digest)=64 AND key_material_digest NOT GLOB '*[^0-9a-f]*'),
    key_registered_revision INTEGER NOT NULL CHECK (key_registered_revision >= 1),
    preparation_key_head_revision INTEGER NOT NULL CHECK (preparation_key_head_revision >= key_registered_revision),
    not_before INTEGER NOT NULL CHECK (not_before >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > not_before),
    mini_project_id TEXT NOT NULL REFERENCES mini_projects(id) ON DELETE RESTRICT,
    task_id TEXT NULL REFERENCES tasks(id) ON DELETE RESTRICT,
    seat_binding_id TEXT NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    topology_node_id TEXT NOT NULL REFERENCES topology_nodes(id) ON DELETE RESTRICT,
    binding_revision INTEGER NOT NULL CHECK (binding_revision >= 1),
    node_revision INTEGER NOT NULL CHECK (node_revision >= 1),
    provenance_domain TEXT NOT NULL CHECK (provenance_domain IN ('hosted','consultation')),
    run_family TEXT NULL CHECK (run_family IN ('advisor','committee','planning_pair')),
    run_id TEXT NULL REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    role_slot_id TEXT NULL,
    run_revision INTEGER NULL CHECK (run_revision >= 1),
    occupancy_generation INTEGER NOT NULL CHECK (occupancy_generation >= 1),
    runtime_kind TEXT NOT NULL CHECK (length(CAST(runtime_kind AS BLOB)) BETWEEN 1 AND 256),
    host TEXT NOT NULL CHECK (length(CAST(host AS BLOB)) BETWEEN 1 AND 256),
    native_generation INTEGER NOT NULL CHECK (native_generation >= 1),
    native_id TEXT NOT NULL CHECK (length(CAST(native_id AS BLOB)) BETWEEN 1 AND 256),
    registered_revision INTEGER NOT NULL CHECK (registered_revision >= 1),
    revoked_revision INTEGER NULL CHECK (revoked_revision > registered_revision),
    PRIMARY KEY (project_id, application, issuer, token_id),
    UNIQUE (project_id, application, registered_revision),
    UNIQUE (project_id, application, revoked_revision),
    FOREIGN KEY (project_id, application, issuer, key_id)
        REFERENCES attestation_authority_keys(project_id, application, issuer, key_id)
        ON DELETE RESTRICT,
    CHECK ((provenance_domain = 'hosted' AND run_family IS NULL AND run_id IS NULL
              AND role_slot_id IS NULL AND run_revision IS NULL)
        OR (provenance_domain = 'consultation' AND run_family IS NOT NULL AND run_id IS NOT NULL
              AND role_slot_id IS NOT NULL AND run_revision IS NOT NULL))
) STRICT;

CREATE TRIGGER attestation_tokens_registration_next
BEFORE INSERT ON prepared_attestation_tokens
WHEN EXISTS (SELECT 1 FROM prepared_attestation_tokens
             WHERE project_id = NEW.project_id AND application = NEW.application
               AND issuer = NEW.issuer AND token_id = NEW.token_id)
  OR NEW.revoked_revision IS NOT NULL OR NEW.registered_revision <>
    COALESCE((SELECT revision FROM attestation_token_heads
              WHERE project_id = NEW.project_id AND application = NEW.application), 0) + 1
BEGIN SELECT RAISE(ABORT, 'token registration requires a new identity and next token head'); END;

-- The public mapping/interval commitment must be coherent with the selected
-- immutable key and current key head. This is still unqualified metadata.
CREATE TRIGGER attestation_tokens_key_commitment BEFORE INSERT ON prepared_attestation_tokens
WHEN NOT EXISTS (SELECT 1 FROM attestation_authority_keys k
    JOIN attestation_authority_heads h ON h.project_id=k.project_id AND h.application=k.application
    WHERE k.project_id=NEW.project_id AND k.application=NEW.application
      AND k.issuer=NEW.issuer AND k.key_id=NEW.key_id AND k.revoked_revision IS NULL
      AND k.material_digest=NEW.key_material_digest AND k.registered_revision=NEW.key_registered_revision
      AND h.revision=NEW.preparation_key_head_revision
      AND NEW.not_before>=k.not_before AND NEW.expires_at<=k.expires_at)
BEGIN SELECT RAISE(ABORT, 'prepared key commitment must match the current immutable key'); END;

CREATE TRIGGER attestation_token_heads_initial BEFORE INSERT ON attestation_token_heads
WHEN EXISTS (SELECT 1 FROM attestation_token_heads
             WHERE project_id = NEW.project_id AND application = NEW.application)
  OR NEW.revision <> 1 OR NOT EXISTS (SELECT 1 FROM prepared_attestation_tokens
      WHERE project_id = NEW.project_id AND application = NEW.application AND registered_revision = 1)
BEGIN SELECT RAISE(ABORT, 'an initial token head requires its first prepared token'); END;

CREATE TRIGGER attestation_token_heads_monotonic BEFORE UPDATE ON attestation_token_heads
WHEN NEW.project_id <> OLD.project_id OR NEW.application <> OLD.application
  OR NEW.revision <> OLD.revision + 1 OR NOT EXISTS (SELECT 1 FROM prepared_attestation_tokens
      WHERE project_id = NEW.project_id AND application = NEW.application
        AND (registered_revision = NEW.revision OR revoked_revision = NEW.revision))
BEGIN SELECT RAISE(ABORT, 'token head requires its exact next mutation'); END;

CREATE TRIGGER attestation_token_heads_permanent BEFORE DELETE ON attestation_token_heads
BEGIN SELECT RAISE(ABORT, 'token heads are permanent'); END;

CREATE TRIGGER prepared_attestation_tokens_immutable BEFORE UPDATE ON prepared_attestation_tokens
WHEN NEW.project_id IS NOT OLD.project_id
  OR NEW.application IS NOT OLD.application
  OR NEW.issuer IS NOT OLD.issuer
  OR NEW.key_id IS NOT OLD.key_id
  OR NEW.token_id IS NOT OLD.token_id
  OR NEW.payload_digest IS NOT OLD.payload_digest
  OR NEW.key_material_digest IS NOT OLD.key_material_digest
  OR NEW.key_registered_revision IS NOT OLD.key_registered_revision
  OR NEW.preparation_key_head_revision IS NOT OLD.preparation_key_head_revision
  OR NEW.not_before IS NOT OLD.not_before
  OR NEW.expires_at IS NOT OLD.expires_at
  OR NEW.mini_project_id IS NOT OLD.mini_project_id
  OR NEW.task_id IS NOT OLD.task_id
  OR NEW.seat_binding_id IS NOT OLD.seat_binding_id
  OR NEW.topology_node_id IS NOT OLD.topology_node_id
  OR NEW.binding_revision IS NOT OLD.binding_revision
  OR NEW.node_revision IS NOT OLD.node_revision
  OR NEW.provenance_domain IS NOT OLD.provenance_domain
  OR NEW.run_family IS NOT OLD.run_family
  OR NEW.run_id IS NOT OLD.run_id
  OR NEW.role_slot_id IS NOT OLD.role_slot_id
  OR NEW.run_revision IS NOT OLD.run_revision
  OR NEW.occupancy_generation IS NOT OLD.occupancy_generation
  OR NEW.runtime_kind IS NOT OLD.runtime_kind
  OR NEW.host IS NOT OLD.host
  OR NEW.native_generation IS NOT OLD.native_generation
  OR NEW.native_id IS NOT OLD.native_id
  OR NEW.registered_revision IS NOT OLD.registered_revision
  OR OLD.revoked_revision IS NOT NULL OR NEW.revoked_revision IS NULL
  OR NEW.revoked_revision IS NOT (SELECT revision + 1 FROM attestation_token_heads
      WHERE project_id = OLD.project_id AND application = OLD.application)
BEGIN SELECT RAISE(ABORT, 'prepared metadata is immutable and revocation is one way'); END;

CREATE TRIGGER prepared_attestation_tokens_permanent BEFORE DELETE ON prepared_attestation_tokens
BEGIN SELECT RAISE(ABORT, 'prepared token identity is permanent'); END;

CREATE TRIGGER attestation_token_registration_head AFTER INSERT ON prepared_attestation_tokens
BEGIN
    INSERT INTO attestation_token_heads (project_id, application, revision)
        SELECT NEW.project_id, NEW.application, 1 WHERE NOT EXISTS (
            SELECT 1 FROM attestation_token_heads WHERE project_id = NEW.project_id
                AND application = NEW.application);
    UPDATE attestation_token_heads SET revision = NEW.registered_revision
        WHERE project_id = NEW.project_id AND application = NEW.application
          AND revision = NEW.registered_revision - 1;
END;
CREATE TRIGGER attestation_token_revocation_head AFTER UPDATE ON prepared_attestation_tokens
BEGIN
    UPDATE attestation_token_heads SET revision = NEW.revoked_revision
        WHERE project_id = NEW.project_id AND application = NEW.application
          AND revision = NEW.revoked_revision - 1;
END;
PRAGMA user_version = 128;
