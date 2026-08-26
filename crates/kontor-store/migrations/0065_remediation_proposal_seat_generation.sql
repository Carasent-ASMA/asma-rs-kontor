-- A remediation proposal is authority evidence, not merely a document hash.
-- Bind historical proposals to generation one and all new proposals to the
-- exact authenticated occupancy. A later native replacement can then fence the
-- old LSA bearer without changing the logical SeatBinding.
ALTER TABLE epic_completion_remediation_proposals
ADD COLUMN lsa_occupancy_generation INTEGER NOT NULL DEFAULT 1
                                          CHECK (lsa_occupancy_generation >= 1);

PRAGMA user_version = 65;
