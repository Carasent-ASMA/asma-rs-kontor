-- Schema v117. The canonical tail each issued message was sent after.
--
-- Reconciling "did this send land?" reads the session's canonical content and
-- looks for the client message id. Finding it is cheap — it is near the tail,
-- because it was sent last. Proving it appears *once* is what is expensive, and
-- that is what the scan actually requires: it walks backward from the tail under
-- a page budget and refuses unless it reached the beginning, because only a
-- complete read can count occurrences across a whole transcript. An exact hit on
-- the first page is therefore found and then discarded.
--
-- The consequence is a ceiling rather than a slowdown. At four pages of five
-- hundred entries, every send into a session past two thousand canonical entries
-- is refused as confirmation-unknown, however healthy the runtime is — which is
-- how seats that had grown large stopped being able to acknowledge messages that
-- had plainly landed and run. Raising the budget does not fix that; it moves the
-- number at which it happens.
--
-- What makes the whole read unnecessary is a floor. A send cannot have landed
-- before it was issued, so the session's tail at the moment of issuance divides
-- the transcript into a part that cannot contain this delivery and a part that
-- might. Everything this reconciliation needs is in the second part, and the
-- second part is bounded by how much the session grew since — not by how long it
-- has been alive.
--
-- So the boundary is recorded here, per issuance, before the effect is
-- attempted. It is a fact about *when Kontor asked*, never about what the
-- runtime did: it is written whether or not the send is accepted, and it never
-- asserts delivery. An occurrence below it belongs to some other issuance, which
-- the primary key on message_id and the delivered position already tell apart,
-- so exact-once is preserved rather than traded away.
--
-- Nullable, because rows written before this migration have no boundary and none
-- can be invented for them: the tail at their issuance is not recoverable, and
-- guessing one would authorize a bounded scan over a range that may not contain
-- the delivery. Those rows keep the existing whole-history requirement and its
-- honest refusal. Nothing is backfilled.

ALTER TABLE runtime_message_issuances
    ADD COLUMN boundary_epoch INTEGER NULL
        CHECK (boundary_epoch IS NULL OR boundary_epoch >= 1);

-- Zero is permitted here and nowhere else in this table: it is the tail of a
-- session that has no content yet, which is a real boundary meaning "nothing
-- precedes this send", not a missing one.
ALTER TABLE runtime_message_issuances
    ADD COLUMN boundary_sequence INTEGER NULL
        CHECK (boundary_sequence IS NULL OR boundary_sequence >= 0);

PRAGMA user_version = 117;
