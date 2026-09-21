-- Publication decisions are audit evidence. Schema v89 made replay unique,
-- but uniqueness alone does not prevent an existing decision from being
-- rewritten or erased by a direct database writer.
CREATE TRIGGER publication_attestations_no_update
BEFORE UPDATE ON publication_attestations
BEGIN
    SELECT RAISE(ABORT, 'a publication attestation is immutable');
END;

CREATE TRIGGER publication_attestations_no_delete
BEFORE DELETE ON publication_attestations
BEGIN
    SELECT RAISE(ABORT, 'a publication attestation is immutable');
END;

PRAGMA user_version = 119;
