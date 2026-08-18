-- Write-time shareability classification for the published documents this
-- schema owns.
--
-- Only tier-B documents gain the stamp. The tier-A operational tables added by
-- 0023 -- topology_nodes, seat_bindings and adaptive_admission_state -- are
-- deliberately left alone: refusing the classification means having nowhere to
-- put it, not storing a null.
--
-- The columns carry defaults so existing rows adopt the tier-B default without
-- a human decision, which is exactly the documented backfill: an older realm
-- opens unchanged and every already-published revision reads back as
-- project_shared by the type-default rule.
--
-- Both tables already refuse UPDATE and DELETE through the triggers created in
-- 0023, so the stamp inherits that immutability and cannot be revised after the
-- fact.

ALTER TABLE topology_specs ADD COLUMN shareability_class TEXT NOT NULL
    DEFAULT 'project_shared'
    CHECK (shareability_class IN ('project_shared', 'kontor_local'));

ALTER TABLE topology_specs ADD COLUMN shareability_classifier TEXT;

ALTER TABLE topology_specs ADD COLUMN shareability_provenance TEXT NOT NULL
    DEFAULT 'type_default'
    CHECK (shareability_provenance IN ('type_default', 'human_override'));

ALTER TABLE role_catalog_revisions ADD COLUMN shareability_class TEXT NOT NULL
    DEFAULT 'project_shared'
    CHECK (shareability_class IN ('project_shared', 'kontor_local'));

ALTER TABLE role_catalog_revisions ADD COLUMN shareability_classifier TEXT;

ALTER TABLE role_catalog_revisions ADD COLUMN shareability_provenance TEXT NOT NULL
    DEFAULT 'type_default'
    CHECK (shareability_provenance IN ('type_default', 'human_override'));

-- An override is attributable and a default rule is not, so the identity column
-- and the provenance column may never disagree. ALTER TABLE cannot add a
-- table-level CHECK across two columns, so the pairing is enforced on insert.

CREATE TRIGGER topology_specs_shareability_is_attributable
BEFORE INSERT ON topology_specs
WHEN (NEW.shareability_provenance = 'human_override')
     <> (NEW.shareability_classifier IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'shareability classifier identity and provenance disagree');
END;

CREATE TRIGGER role_catalog_shareability_is_attributable
BEFORE INSERT ON role_catalog_revisions
WHEN (NEW.shareability_provenance = 'human_override')
     <> (NEW.shareability_classifier IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'shareability classifier identity and provenance disagree');
END;

PRAGMA user_version = 25;
