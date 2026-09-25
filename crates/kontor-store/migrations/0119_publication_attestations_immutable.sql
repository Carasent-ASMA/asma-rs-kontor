-- Schema v119. Recorded publication decisions are append-only evidence (ASMA-8102).
--
-- Migration 0089 created the ledger without update/delete guards. A maintenance
-- path with database write access could falsify or erase the exact evidence
-- publication consumers and later audits trust.

CREATE TRIGGER publication_attestation_no_update
BEFORE UPDATE ON publication_attestations
BEGIN SELECT RAISE(ABORT, 'a recorded publication attestation is immutable evidence'); END;

CREATE TRIGGER publication_attestation_no_delete
BEFORE DELETE ON publication_attestations
BEGIN SELECT RAISE(ABORT, 'a recorded publication attestation is not deletable'); END;

PRAGMA user_version = 119;
