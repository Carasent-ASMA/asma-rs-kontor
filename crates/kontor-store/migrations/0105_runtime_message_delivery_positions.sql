-- Schema v105. Where each issued client message actually landed.
--
-- v104 records that Kontor issued an id exactly once, to exactly one binding,
-- and a bounded observation trusts that instead of counting occurrences across
-- a whole transcript. Issuance is not occurrence, though, and the difference is
-- the hole: Kontor issuing an id once says nothing about how many times the
-- runtime's canonical content mentions it. A runtime that echoes a message
-- produces two occurrences of an id that was issued once, and an observation
-- bounded to a tail window sees only the newer of them — the earlier one is
-- outside the window, so uniqueness "holds" by not looking.
--
-- Counting occurrences is what the bound removed and must not be reintroduced,
-- so the question is answered from the other side. The delivery acknowledgement
-- already reports the position the message landed at, and that position is a
-- fact about *this* delivery rather than about the transcript. Recorded here, it
-- turns "is this the only occurrence?" — which needs a scan — into "is this the
-- occurrence Kontor delivered?", which is a primary-key lookup and a
-- comparison.
--
-- Nullable, because the row is written before the runtime is asked to accept the
-- message and there is no position to record yet. A row that never gains one is
-- a delivery whose acknowledgement was lost, and an observation refuses it
-- rather than assuming which occurrence was meant: the operator sends a new
-- correlation message, exactly as for an id issued before the ledger existed.

ALTER TABLE runtime_message_issuances
    ADD COLUMN delivered_epoch INTEGER NULL CHECK (delivered_epoch IS NULL OR delivered_epoch >= 1);

ALTER TABLE runtime_message_issuances
    ADD COLUMN delivered_sequence INTEGER NULL
        CHECK (delivered_sequence IS NULL OR delivered_sequence >= 1);

PRAGMA user_version = 105;
